use log::{debug, error};
use std::{
    io::{self, Read},
    str::FromStr,
};

use anyhow::{Result, bail};
use clap::Parser;
use quick_xml::{Reader, events::Event};
use svgtypes::{SimplifyingPathParser, Transform};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    input: String,
}

fn transform_multiply(ts1: &Transform, ts2: &Transform) -> Transform {
    Transform {
        a: ts1.a * ts2.a + ts1.c * ts2.b,
        b: ts1.b * ts2.a + ts1.d * ts2.b,
        c: ts1.a * ts2.c + ts1.c * ts2.d,
        d: ts1.b * ts2.c + ts1.d * ts2.d,
        e: ts1.a * ts2.e + ts1.c * ts2.f + ts1.e,
        f: ts1.b * ts2.e + ts1.d * ts2.f + ts1.f,
    }
}

fn apply_transform(coord: (f64, f64), t: &Transform) -> (f64, f64) {
    let (x, y) = coord;
    (t.a * x + t.c * y + t.e, t.b * x + t.d * y + t.f)
}

#[derive(Debug, Default, Clone)]
struct SvgStyle {
    pub fill: Option<String>,
    pub fill_rule: Option<String>,
    pub stroke_width: Option<f64>,
    pub stroke: Option<String>,
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub dash_array: Option<String>,
}

impl SvgStyle {
    pub fn format_fill(&self) {
        if let Some(fill) = &self.fill {
            print!("fill: {}, ", fill);
        }
    }
    pub fn format_stroke(&self) {
        if self.stroke.is_some() || self.stroke_width.is_some() || self.dash_array.is_some() {
            print!("stroke: (");
            if let Some(stroke) = &self.stroke {
                print!("paint: {}, ", stroke);
            }
            if let Some(thickness) = self.stroke_width {
                print!("thickness: {}pt,", thickness);
            }
            if self.dash_array.is_some() {
                print!("dash: \"dashed\",")
            }
            print!("),");
        } else {
            print!("stroke: none, ")
        }
    }
}

impl FromStr for SvgStyle {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut r = SvgStyle::default();
        for kv_str in s.split(';') {
            let mut split = kv_str.split(':');
            if let Some(key) = split.next()
                && let Some(value) = split.next()
            {
                if key == "fill" {
                    r.fill = Some(value.to_string());
                } else if key == "fill-rule" {
                    r.fill_rule = Some(value.to_string());
                } else if key == "stroke-width" {
                    r.stroke_width = Some(f64::from_str(&value[..value.len() - 2])?);
                } else if key == "stroke" {
                    r.stroke = Some(value.to_string());
                } else if key == "font-family" {
                    r.font_family = Some(value.to_string());
                } else if key == "font-size" {
                    if value.ends_with("px") {
                        r.font_size = Some(f64::from_str(&value[..value.len() - 2])?);
                    } else {
                        r.font_size = Some(f64::from_str(&value)?);
                    }
                } else if key == "stroke-dasharray" {
                    r.dash_array = Some(value.to_string());
                } else {
                    debug!("Unprocessed style: {}", kv_str);
                }
            } else if !kv_str.is_empty() {
                return Err(anyhow::anyhow!("unexpected format {}", kv_str));
            }
        }
        Ok(r)
    }
}

fn gen_content(pos: (f64, f64), style: &Option<SvgStyle>, text_content: &str, font_scale: f64) {
    let (x1, y1) = pos;
    print!("content(({},{}), ", x1, y1);
    print!("anchor: \"south-west\",");
    if let Some(style) = style {
        print!("text(");
        if let Some(font_size) = style.font_size {
            print!("size: {}pt, ", font_size * font_scale);
        }
        if let Some(font_family) = &style.font_family {
            print!(
                "font: ({}, ), ",
                font_family.replace("'", "\"").replace(", monospace", "")
            );
        }
        if let Some(fill) = &style.fill
            && fill != "none"
        {
            print!("fill: {}, ", fill);
        }
        print!(")")
    }
    print!(
        "[{}]",
        text_content
            .replace("$", "\\$")
            .replace("[", "\\[")
            .replace("]", "\\]")
            .replace("/", "\\/")
            .replace("#", "\\#")
    );

    print!(")\n");
}

#[derive(Debug, Default, Clone)]
struct EventEntry {
    name: Vec<u8>,
    transform: Transform,
    // tspan may have multiple
    positions: Option<Vec<(f64, f64)>>,
    style: Option<SvgStyle>,
}

fn handle_event(
    reader: &mut Reader<&[u8]>,
    root_transform: &Transform,
    font_scale: f64,
) -> Result<()> {
    let mut events_stack = vec![EventEntry {
        name: Vec::from(b"root"),
        transform: root_transform.clone(),
        positions: Default::default(),
        style: Default::default(),
    }];
    let mut event_buf = Vec::new();
    loop {
        let event = reader.read_event_into(&mut event_buf)?;
        match event {
            Event::Eof => {
                break;
            }
            Event::End(element) => {
                events_stack.pop_if(|item| &item.name == element.name().as_ref());
            }
            Event::Start(element) => {
                if element.name().as_ref() == b"g" {
                    for attr_result in element.attributes() {
                        let a = attr_result?;
                        let mut cur_transform = events_stack.last().unwrap().transform.clone();
                        match a.key.as_ref() {
                            b"transform" => {
                                let transform_str =
                                    a.decode_and_unescape_value(reader.decoder())?;
                                debug!("transform_str: {}", transform_str);
                                cur_transform = transform_multiply(
                                    &cur_transform,
                                    &Transform::from_str(transform_str.as_ref())?,
                                );
                                debug!("cur_transform {:?}", cur_transform);
                                // handle_event(sub_event, reader, &cur_transform)?;
                            }
                            _ => debug!(
                                "Unprocessed attr for <g> {}",
                                str::from_utf8(a.key.as_ref())?
                            ),
                        }
                        events_stack.push(EventEntry {
                            name: Vec::from(element.name().as_ref()),
                            transform: cur_transform,
                            positions: None,
                            style: None,
                        });
                        // loop {
                        //     let sub_event = reader.read_event_into(&mut event_buf)?;
                        //     if let Event::End(ref e) = sub_event
                        //         && e.name().as_ref() == b"g"
                        //     {
                        //         debug!("End of g :{:?}", sub_event);
                        //         break;
                        //     }
                        //     handle_event(sub_event, reader, &cur_transform, font_scale)?;
                        // }
                    }
                } else if element.name().as_ref() == b"text" {
                    let mut x = 0.0;
                    let mut y = 0.0;
                    let mut style = None;
                    for attr in element.attributes() {
                        let a = attr?;
                        let val_cow = a.decode_and_unescape_value(reader.decoder())?;
                        let val_str = val_cow.as_ref();
                        match a.key.as_ref() {
                            b"x" => {
                                x = if val_str.ends_with("px") {
                                    f64::from_str(&val_str[0..val_str.len() - 2])?
                                } else {
                                    f64::from_str(val_str)?
                                };
                            }
                            b"y" => {
                                y = if val_str.ends_with("px") {
                                    f64::from_str(&val_str[0..val_str.len() - 2])?
                                } else {
                                    f64::from_str(val_str)?
                                };
                            }
                            b"style" => {
                                style = Some(SvgStyle::from_str(val_str)?);
                            }
                            _ => debug!(
                                "Unprocessed attributes for <text> {}",
                                str::from_utf8(a.key.as_ref())?
                            ),
                        }
                    }
                    let last_transform = events_stack.last().unwrap().transform.clone();
                    events_stack.push(EventEntry {
                        name: Vec::from(element.name().as_ref()),
                        transform: last_transform,
                        positions: Some(vec![(x, y)]),
                        style: style,
                    });
                    // loop {
                    //     let evt = reader.read_event_into(&mut event_buf)?;
                    //     if let Event::End(element) = &evt
                    //         && element.name().as_ref() == b"text"
                    //     {
                    //         num_text_end_expected -= 1;
                    //         if num_text_end_expected == 0 {
                    //             break;
                    //         }
                    //     }
                    //     // println!("---- {:?}", evt);
                    //     if let Event::Text(content) = &evt {
                    //         text_content.push_str(str::from_utf8(content.as_ref())?);
                    //     }
                    // }
                    // debug!(
                    //     "text --- x = {}, y = {}, style = {:?}, content = {}",
                    //     x, y, style, text_content
                    // );
                    // let (x1, y1) = apply_transform((x, y), transform);
                    // gen_content(x1, y1, &style, &text_content, font_scale);
                } else if element.name().as_ref() == b"tspan" {
                    let mut x = Vec::<f64>::new();
                    let mut y = Vec::<f64>::new();
                    for attr in element.attributes() {
                        let a = attr?;
                        let val_cow = a.decode_and_unescape_value(reader.decoder())?;
                        let val_str = val_cow.as_ref();
                        match a.key.as_ref() {
                            b"x" => {
                                x = val_str
                                    .split_whitespace()
                                    .filter(|i| !i.is_empty())
                                    .map(|i| {
                                        if i.ends_with("px") {
                                            f64::from_str(&i[0..i.len() - 2]).unwrap()
                                        } else {
                                            f64::from_str(i).unwrap()
                                        }
                                    })
                                    .collect();
                            }
                            b"y" => {
                                y = val_str
                                    .split_whitespace()
                                    .filter(|i| !i.is_empty())
                                    .map(|i| {
                                        if i.ends_with("px") {
                                            f64::from_str(&i[0..i.len() - 2]).unwrap()
                                        } else {
                                            f64::from_str(i).unwrap()
                                        }
                                    })
                                    .collect();
                            }
                            _ => debug!(
                                "Unprocessed attributes for <text> {}",
                                str::from_utf8(a.key.as_ref())?
                            ),
                        }
                    }
                    let mut style = None;
                    for e in events_stack.iter().rev() {
                        if let Some(s) = &e.style {
                            style = Some(s.clone());
                            break;
                        }
                    }
                    events_stack.push(EventEntry {
                        name: Vec::from(element.name().as_ref()),
                        transform: events_stack.last().unwrap().transform.clone(),
                        positions: Some(x.iter().zip(y.iter()).map(|(i, j)| (*i, *j)).collect()),
                        style,
                    });
                } else {
                    debug!(
                        "Unprocessed Event::Start {}",
                        str::from_utf8(element.name().as_ref())?
                    );
                }
            }
            Event::Text(text_content) => {
                if let Some(parent) = events_stack
                    .iter()
                    .rev()
                    .find(|i| i.name == b"text" || i.name == b"tspan")
                {
                    if let Some(positions) = &parent.positions
                        && !positions.is_empty()
                    {
                        if parent.name == b"text" {
                            gen_content(
                                apply_transform(
                                    positions[0].clone(),
                                    &events_stack.last().unwrap().transform,
                                ),
                                &parent.style,
                                str::from_utf8(text_content.as_ref())?,
                                font_scale,
                            );
                        }
                        if parent.name == b"tspan" {
                            if positions.len() > 1 {
                                for (ch, pos) in text_content.as_ref().iter().zip(positions.iter())
                                {
                                    gen_content(
                                        apply_transform(
                                            pos.clone(),
                                            &events_stack.last().unwrap().transform,
                                        ),
                                        &parent.style,
                                        str::from_utf8(&[*ch])?,
                                        font_scale,
                                    );
                                }
                            } else {
                                gen_content(
                                    apply_transform(
                                        positions[0].clone(),
                                        &events_stack.last().unwrap().transform,
                                    ),
                                    &parent.style,
                                    str::from_utf8(text_content.as_ref())?,
                                    font_scale,
                                );
                            }
                        }
                    } else {
                        bail!("No positions found for text!");
                    }
                } else {
                    return Err(anyhow::anyhow!(
                        "Can't find parent for text {:?}",
                        text_content
                    ));
                }
            }
            Event::Empty(element) => {
                let parent = events_stack.last().unwrap();
                match element.name().as_ref() {
                    b"rect" => {
                        let mut x = 0.0;
                        let mut y = 0.0;
                        let mut width = 0.0;
                        let mut height = 0.0;
                        let mut style = Default::default();
                        for attr in element.attributes() {
                            let a = attr?;
                            let val_cow = a.decode_and_unescape_value(reader.decoder())?;
                            let val_str = val_cow.as_ref();
                            match a.key.as_ref() {
                                b"x" => {
                                    x = f64::from_str(val_str)?;
                                }
                                b"y" => {
                                    y = f64::from_str(val_str)?;
                                }
                                b"width" => {
                                    width = f64::from_str(val_str)?;
                                }
                                b"height" => {
                                    height = f64::from_str(val_str)?;
                                }
                                b"style" => {
                                    style = SvgStyle::from_str(val_str)?;
                                }
                                _ => debug!(
                                    "Unprocessed attributes for <rect> {}",
                                    str::from_utf8(a.key.as_ref())?
                                ),
                            }
                        }
                        let (x1, y1) = apply_transform((x, y), &parent.transform);
                        let (x2, y2) = apply_transform((x + width, y + height), &parent.transform);
                        print!("rect(({}, {}), ({}, {}), ", x1, y1, x2, y2);
                        style.format_fill();
                        style.format_stroke();
                        print!(")\n");
                    }
                    b"path" => {
                        let mut path_segments = None;
                        let mut style = None;
                        for attr in element.attributes() {
                            let a = attr?;
                            let val_str = a.decode_and_unescape_value(reader.decoder())?;
                            match a.key.as_ref() {
                                b"d" => {
                                    let mut segments = Vec::new();
                                    let mut parser = SimplifyingPathParser::from(val_str.as_ref());
                                    while let Some(path_segment) = parser.next() {
                                        segments.push(path_segment?);
                                    }
                                    path_segments = Some(segments);
                                }
                                b"style" => {
                                    style = Some(SvgStyle::from_str(val_str.as_ref())?);
                                }
                                _ => {
                                    debug!("unprocessed attr {:?}", a);
                                }
                            }
                        }
                        debug!("d={:?}, style={:?}", path_segments, style);
                        if let Some(segments) = &path_segments {
                            let mut last_point = (0.0, 0.0);
                            let mut merge_path = false;
                            if let Some(style) = &style
                                && let Some(fill) = &style.fill
                                && fill != "none"
                            {
                                merge_path = true;
                                print!("merge-path(");
                                style.format_fill();
                                style.format_stroke();
                                print!("{{\n");
                            }
                            for s in segments {
                                match s {
                                    svgtypes::SimplePathSegment::MoveTo { x, y } => {
                                        last_point = apply_transform((*x, *y), &parent.transform);
                                    }
                                    svgtypes::SimplePathSegment::LineTo { x, y } => {
                                        let (x, y) = apply_transform((*x, *y), &parent.transform);
                                        print!(
                                            "line(({}, {}), ({}, {}),",
                                            last_point.0, last_point.1, x, y
                                        );
                                        if let Some(style) = &style {
                                            style.format_stroke();
                                        }
                                        print!(")\n");
                                        last_point = (x, y);
                                    }
                                    svgtypes::SimplePathSegment::CurveTo {
                                        x1,
                                        y1,
                                        x2,
                                        y2,
                                        x,
                                        y,
                                    } => {
                                        let (x1, y1) =
                                            apply_transform((*x1, *y1), &parent.transform);
                                        let (x2, y2) =
                                            apply_transform((*x2, *y2), &parent.transform);
                                        let (x, y) = apply_transform((*x, *y), &parent.transform);
                                        print!(
                                            "bezier(({}, {}), ({}, {}), ({}, {}), ({}, {}),",
                                            last_point.0, last_point.1, x, y, x1, y1, x2, y2,
                                        );
                                        if let Some(style) = &style {
                                            style.format_stroke();
                                        }
                                        print!(")\n");
                                        last_point = (x, y);
                                    }
                                    svgtypes::SimplePathSegment::ClosePath => {}
                                    _ => todo!(),
                                }
                            }
                            if merge_path {
                                println!("}})");
                            }
                        }
                    }
                    b"ellipse" => {
                        let mut cx = 0.0;
                        let mut cy = 0.0;
                        let mut rx = 0.0;
                        let mut ry = 0.0;
                        let mut style = Default::default();
                        for attr in element.attributes() {
                            let a = attr?;
                            let val_cow = a.decode_and_unescape_value(reader.decoder())?;
                            let val_str = val_cow.as_ref();
                            match a.key.as_ref() {
                                b"cx" => {
                                    cx = f64::from_str(val_str)?;
                                }
                                b"cy" => {
                                    cy = f64::from_str(val_str)?;
                                }
                                b"rx" => {
                                    rx = f64::from_str(val_str)?;
                                }
                                b"ry" => {
                                    ry = f64::from_str(val_str)?;
                                }
                                b"style" => {
                                    style = Some(SvgStyle::from_str(val_str)?);
                                }
                                _ => debug!(
                                    "Unprocessed attributes for <rect> {}",
                                    str::from_utf8(a.key.as_ref())?
                                ),
                            }
                        }
                        // println!("{:?}", transform);
                        let (cx1, cy1) = apply_transform((cx, cy), &parent.transform);
                        let (rx1, ry1) = apply_transform((cx + rx, cy + ry), &parent.transform);
                        print!(
                            "circle(({}, {}), radius: ({}, {}), ",
                            cx1,
                            cy1,
                            rx1 - cx1,
                            ry1 - cy1,
                        );
                        if let Some(style) = &style {
                            style.format_fill();
                            style.format_stroke();
                        }
                        print!(")\n")
                    }
                    b"circle" => {
                        let mut cx = 0.0;
                        let mut cy = 0.0;
                        let mut r = 0.0;
                        let mut style = Default::default();
                        for attr in element.attributes() {
                            let a = attr?;
                            let val_cow = a.decode_and_unescape_value(reader.decoder())?;
                            let val_str = val_cow.as_ref();
                            match a.key.as_ref() {
                                b"cx" => {
                                    cx = f64::from_str(val_str)?;
                                }
                                b"cy" => {
                                    cy = f64::from_str(val_str)?;
                                }
                                b"r" => {
                                    r = f64::from_str(val_str)?;
                                }
                                b"style" => {
                                    style = Some(SvgStyle::from_str(val_str)?);
                                }
                                _ => debug!(
                                    "Unprocessed attributes for <rect> {}",
                                    str::from_utf8(a.key.as_ref())?
                                ),
                            }
                        }
                        // println!("{:?}", transform);
                        let (cx1, cy1) = apply_transform((cx, cy), &parent.transform);
                        let (rx1, _) = apply_transform((cx + r, cy + r), &parent.transform);
                        print!("circle(({}, {}), radius: {}, ", cx1, cy1, rx1 - cx1,);
                        if let Some(style) = &style {
                            style.format_fill();
                            style.format_stroke();
                        }
                        print!(")\n")
                    }
                    _ => debug!("Unprocessed element: {:?}", element),
                }
            }
            _ => {
                debug!("Unhandled event: {:?}", event);
            }
        }
    }
    Ok(())
}

fn convert(reader: &mut Reader<&[u8]>, transform: &Transform, font_scale: f64) -> Result<()> {
    handle_event(reader, transform, font_scale)
}

fn main() -> Result<()> {
    env_logger::init();
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let mut reader = Reader::from_str(&input);
    reader.config_mut().trim_text(true);
    convert(
        &mut reader,
        &Transform::new(0.01, 0.0, 0.0, -0.01, 0.0, 0.0),
        // &Transform::default(),
        0.27,
    )
}
