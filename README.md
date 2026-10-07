# svg2cetz

Converts SVG files into [CeTZ](https://github.com/cetz-package/cetz) drawing code
for [Typst](https://typst.app).

## Usage

```sh
# stdin -> stdout
svg2cetz < input.svg > out.typ

# file -> file, wrapped in a standalone Typst document
svg2cetz --standalone input.svg -o out.typ
```

The generated body is a sequence of `cetz.draw` calls. Without `--standalone`
you paste it into your own canvas:

```typ
#import "@preview/cetz:0.5.2"
#cetz.canvas(length: 1cm, {
  import cetz.draw: *
  // ...generated code...
})
```

### Flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `input` | stdin | Input SVG file |
| `-o, --output` | stdout | Output file |
| `-s, --scale` | `0.003` | SVG user units → CeTZ units |
| `-f, --font-scale` | `25` | Multiplier for font sizes |
| `--px-scale` | `30` | Multiplier for stroke widths / dash patterns |
| `--fit <n>` | off | Auto-scale so the viewBox's longest edge is `n` units (overrides `--scale`) |
| `--standalone` | off | Emit the `#import` + `#cetz.canvas` wrapper |

Coordinates are normalised against the SVG `viewBox` (its origin maps to
`(0, 0)`) and the y axis is flipped, because CeTZ is y-up while SVG is y-down.

## Supported SVG subset

- Shapes: `path` (incl. arcs, quadratics, multi-subpath fills/holes), `rect`,
  `circle`, `ellipse`, `line`, `polyline`, `polygon`
- Reuse: `<use href="#id">` and `<symbol>` (with viewBox→width/height
  scaling), including `x`/`y` offsets and transforms
- Grouping/transforms: nested `<g>` and per-element `transform`
  (`translate`/`scale`/`rotate`/`matrix`/`skew`)
- Styling: presentation attributes and `style=` (the latter wins), with
  inheritance — `fill`, `stroke`, `stroke-width`, `stroke-dasharray`,
  `stroke-linecap`, `stroke-linejoin`, `stroke-miterlimit`, `fill-rule`,
  `opacity`, `fill-opacity`, `stroke-opacity`. Element and group `opacity`
  are folded into paint alpha, since CeTZ has no group opacity.
- Colours: hex, `rgb()`, and the 148 named SVG colours; opacity is mapped to
  Typst colour alpha (`transparentize`)
- Gradients: `linearGradient`/`radialGradient` with `objectBoundingBox` units
  map to native Typst `gradient.linear`/`gradient.radial`
- Text: `<text>`/`<tspan>` incl. per-glyph `x`/`y` lists, `font-family`,
  `font-size`, `text-anchor`, `dominant-baseline`

## Known limitations

- `userSpaceOnUse` gradients and `gradientTransform` fall back to a solid
  colour (a warning is printed)
- `<image>`, filters, masks, patterns and `clip-path` are not rendered
- `dominant-baseline` is honoured only for `central`/`middle`

## Development

```sh
cargo build --release
cargo test          # unit tests + fixture conversion + golden snapshots
./scripts/verify.sh # converts every fixture and compiles it with typst
```

`scripts/verify.sh` is the regression gate: it requires `typst` on PATH with
`@preview/cetz:0.5.2` cached, and fails if any fixture's output stops
compiling. Fixtures live in `tests/fixtures/`.

Golden CeTZ snapshots in `tests/snapshots/` pin the output format so
regressions are caught even without typst; regenerate them with
`scripts/gen-snapshots.sh` after an intentional format change.
