use std::{
    borrow::Cow,
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
    pub stroke_width: Option<f64>,
    pub stroke: Option<String>,
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
                } else if key == "stroke-width" {
                    r.stroke_width = Some(f64::from_str(&value[..value.len() - 2])?);
                } else if key == "stroke" {
                    r.stroke = Some(value.to_string());
                } else {
                    println!("Unprocessed style: {}", kv_str);
                }
            } else if !kv_str.is_empty() {
                return Err(anyhow::anyhow!("unexpected format {}", kv_str));
            }
        }
        Ok(r)
    }
}

fn handle_event(event: Event, reader: &mut Reader<&[u8]>, transform: &Transform) -> Result<()> {
    match event {
        Event::Start(element) => {
            if element.name().as_ref() == b"g" {
                let mut event_buf = Vec::new();
                for attr_result in element.attributes() {
                    let a = attr_result?;
                    let mut cur_transform = transform.clone();
                    match a.key.as_ref() {
                        b"transform" => {
                            let transform_str = a.decode_and_unescape_value(reader.decoder())?;
                            println!("transform_str: {}", transform_str);
                            cur_transform = transform_multiply(
                                &cur_transform,
                                &Transform::from_str(transform_str.as_ref())?,
                            );
                            println!("cur_transform {:?}", cur_transform);
                            // handle_event(sub_event, reader, &cur_transform)?;
                        }
                        _ => println!(
                            "Unprocessed attr for <g> {}",
                            str::from_utf8(a.key.as_ref())?
                        ),
                    }
                    loop {
                        let sub_event = reader.read_event_into(&mut event_buf)?;
                        if let Event::End(ref e) = sub_event
                            && e.name().as_ref() == b"g"
                        {
                            println!("End of g :{:?}", sub_event);
                            break;
                        }
                        handle_event(sub_event, reader, &cur_transform)?;
                    }
                }
            }

            Ok(())
        }
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
                            _ => println!(
                                "Unprocessed attributes for <rect> {}",
                                str::from_utf8(a.key.as_ref())?
                            ),
                        }
                    }
                    let (x1, y1) = apply_transform((x, y), transform);
                    let (x2, y2) = apply_transform((x + width, y + height), transform);
                    print!("rect(({}, {}), ({}, {}), ", x1, y1, x2, y2);
                    if let Some(fill) = &style.fill {
                        print!("fill: {},", fill);
                    }
                    if style.stroke.is_some() || style.stroke_width.is_some() {
                        print!("stroke: (");
                        if let Some(stroke) = &style.stroke {
                            print!("paint: {},", stroke);
                        }
                        if let Some(width) = &style.stroke_width {
                            print!("thickness: {}pt, ", width);
                        }
                        print!("),");
                    }
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
                                println!("unprocessed attr {:?}", a);
                            }
                        }
                    }
                    println!("d={:?}, style={:?}", path_segments, style);
                    if let Some(segments) = path_segments {
                        let points: Vec<(f64, f64)> = segments
                            .iter()
                            .map(|i| match i {
                                svgtypes::SimplePathSegment::MoveTo { x, y } => (*x, *y),
                                svgtypes::SimplePathSegment::LineTo { x, y } => (*x, *y),
                                _ => {
                                    unimplemented!()
                                }
                            })
                            .collect();
                        print!("line(");
                        for p in points {
                            let (x, y) = apply_transform(p, transform);
                            print!("({}, {}),", x, y);
                        }
                        print!(")\n");
                    }
                }
                _ => println!("Unprocessed element: {:?}", element),
            }
            Ok(())
        }
        Event::Eof => Ok(()),
        _ => {
            println!("Unhandled event: {:?}", event);
            Ok(())
        }
    }
}

fn convert(reader: &mut Reader<&[u8]>, transform: &Transform) -> Result<()> {
    loop {
        let event = reader.read_event()?;
        if event == Event::Eof {
            break;
        } else {
            handle_event(event, reader, transform)?;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let mut reader = Reader::from_str(&input);
    reader.config_mut().trim_text(true);
    convert(&mut reader, &Transform::new(0.01, 0.0, 0.0, 0.01, 0.0, 0.0))
}
