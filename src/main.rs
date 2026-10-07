mod dom;
mod emit;
mod geom;
mod gradient;
mod path;
mod style;
mod svg;

use std::{fs, io, path::PathBuf};
use std::io::Read;

use anyhow::Result;
use clap::Parser;

use crate::dom::Node;
use crate::emit::Emitter;
use crate::geom::root_transform;
use crate::style::parse_number;

const CETZ_VERSION: &str = "0.5.2";

#[derive(Parser, Debug)]
#[command(version, about = "Convert SVG to CeTZ (Typst) drawing code", long_about = None)]
struct Args {
    /// Input SVG file (reads stdin when omitted)
    input: Option<PathBuf>,

    /// Output file (writes stdout when omitted)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Uniform scale from SVG user units to CeTZ units
    #[arg(short, long, default_value_t = 0.003)]
    scale: f64,

    /// Multiplier applied to font sizes
    #[arg(short = 'f', long, default_value_t = 25.0)]
    font_scale: f64,

    /// Multiplier applied to stroke widths and dash patterns
    #[arg(long, default_value_t = 30.0)]
    px_scale: f64,

    /// Auto-scale the viewBox so its longest edge maps to this many CeTZ
    /// units (overrides --scale)
    #[arg(long)]
    fit: Option<f64>,

    /// Wrap the output in a standalone Typst document with the cetz import
    /// and canvas block
    #[arg(long)]
    standalone: bool,
}

/// `(min-x, min-y, width, height)` of the document viewBox.
fn viewbox(dom: &Node) -> (f64, f64, f64, f64) {
    let svg = dom.children.iter().find(|c| c.tag == "svg");
    let svg = match svg {
        Some(s) => s,
        None => return (0.0, 0.0, 0.0, 0.0),
    };
    if let Some(vb) = svg.attr("viewBox") {
        let n: Vec<f64> = vb.split_whitespace().filter_map(parse_number).collect();
        if n.len() == 4 {
            return (n[0], n[1], n[2], n[3]);
        }
    }
    let w = svg.attr("width").and_then(parse_number).unwrap_or(0.0);
    let h = svg.attr("height").and_then(parse_number).unwrap_or(0.0);
    (0.0, 0.0, w, h)
}

fn wrap_standalone(body: &str) -> String {
    format!(
        "#import \"@preview/cetz:{}\"\n#cetz.canvas(length: 1cm, {{\n  import cetz.draw: *\n{}}})\n",
        CETZ_VERSION, body
    )
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    let input = match &args.input {
        Some(p) => fs::read_to_string(p)?,
        None => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s)?;
            s
        }
    };

    let dom = Node::build(&input)?;
    let gradients = gradient::collect(&dom);
    let (minx, miny, w, h) = viewbox(&dom);

    let scale = match args.fit {
        Some(target) if w.max(h) > 0.0 => target / w.max(h),
        _ => args.scale,
    };

    let root_t = root_transform(minx, miny, h, scale);
    let mut emitter = Emitter::new(gradients, args.px_scale, args.font_scale);
    svg::render(&dom, &mut emitter, &root_t);

    let body = emitter.out;
    let final_out = if args.standalone {
        wrap_standalone(&body)
    } else {
        body
    };

    match &args.output {
        Some(p) => fs::write(p, final_out)?,
        None => {
            let stdout = io::stdout();
            let mut lock = stdout.lock();
            io::Write::write_all(&mut lock, final_out.as_bytes())?;
        }
    }
    Ok(())
}
