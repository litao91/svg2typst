use log::debug;
use std::{
    io::{self, Read},
    str::FromStr,
};

use anyhow::Result;
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

#[derive(Debug, Default)]
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

fn handle_event(
    event: Event,
    reader: &mut Reader<&[u8]>,
    transform: &Transform,
    font_scale: f64,
) -> Result<()> {
    let mut event_buf = Vec::new();
    match event {
        Event::Start(element) => {
            if element.name().as_ref() == b"g" {
                for attr_result in element.attributes() {
                    let a = attr_result?;
                    let mut cur_transform = transform.clone();
                    match a.key.as_ref() {
                        b"transform" => {
                            let transform_str = a.decode_and_unescape_value(reader.decoder())?;
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
                    loop {
                        let sub_event = reader.read_event_into(&mut event_buf)?;
                        if let Event::End(ref e) = sub_event
                            && e.name().as_ref() == b"g"
                        {
                            debug!("End of g :{:?}", sub_event);
                            break;
                        }
                        handle_event(sub_event, reader, &cur_transform, font_scale)?;
                    }
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
                let mut text_content = String::new();
                let mut num_text_end_expected = 1;
                loop {
                    let evt = reader.read_event_into(&mut event_buf)?;
                    if let Event::End(element) = &evt
                        && element.name().as_ref() == b"text"
                    {
                        num_text_end_expected -= 1;
                        if num_text_end_expected == 0 {
                            break;
                        }
                    }
                    if let Event::Text(content) = &evt {
                        text_content.push_str(str::from_utf8(content.as_ref())?);
                    }
                }
                debug!(
                    "text --- x = {}, y = {}, style = {:?}, content = {}",
                    x, y, style, text_content
                );
                let (x1, y1) = apply_transform((x, y), transform);
                print!("content(({},{}), ", x1, y1);
                print!("anchor: \"south-west\",");
                if let Some(style) = style {
                    print!("text(");
                    if let Some(font_size) = style.font_size {
                        print!("size: {}pt, ", font_size * font_scale);
                    }
                    if let Some(font_family) = style.font_family {
                        print!(
                            "font: ({}, ), ",
                            font_family.replace("'", "\"").replace(", monospace", "")
                        );
                    }
                    if let Some(fill) = style.fill
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
                );

                print!(")\n");
            } else {
                debug!(
                    "Unprocessed Event::Start {}",
                    str::from_utf8(element.name().as_ref())?
                );
            }

            Ok(())
        }
        // Event::Text(text_element) => {
        //     debug!("Text {:?}", text_element);
        //     Ok(())
        // }
        Event::Empty(element) => {
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
                    let (x1, y1) = apply_transform((x, y), transform);
                    let (x2, y2) = apply_transform((x + width, y + height), transform);
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
                                    last_point = apply_transform((*x, *y), transform);
                                }
                                svgtypes::SimplePathSegment::LineTo { x, y } => {
                                    let (x, y) = apply_transform((*x, *y), transform);
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
                                    let (x1, y1) = apply_transform((*x1, *y1), transform);
                                    let (x2, y2) = apply_transform((*x2, *y2), transform);
                                    let (x, y) = apply_transform((*x, *y), transform);
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
                    let (cx1, cy1) = apply_transform((cx, cy), transform);
                    let (rx1, ry1) = apply_transform((cx + rx, cy + ry), transform);
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
                    let (cx1, cy1) = apply_transform((cx, cy), transform);
                    let (rx1, _) = apply_transform((cx + r, cy + r), transform);
                    print!("circle(({}, {}), radius: {}, ", cx1, cy1, rx1 - cx1,);
                    if let Some(style) = &style {
                        style.format_fill();
                        style.format_stroke();
                    }
                    print!(")\n")
                }
                _ => debug!("Unprocessed element: {:?}", element),
            }
            Ok(())
        }
        Event::Eof => Ok(()),
        _ => {
            debug!("Unhandled event: {:?}", event);
            Ok(())
        }
    }
}

fn convert(reader: &mut Reader<&[u8]>, transform: &Transform, font_scale: f64) -> Result<()> {
    let mut buf = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf)?;
        if event == Event::Eof {
            break;
        } else {
            handle_event(event, reader, transform, font_scale)?;
        }
    }
    Ok(())
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
