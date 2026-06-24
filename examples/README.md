# Zenith examples

Each `.zen` here is a runnable, validated document; the `.png` beside it is its
rendered output (committed so you can see the result without running anything).
Every example is covered by a determinism test in
`zenith-cli/tests/render_integration.rs` — rendered twice, asserted byte-identical.

Render any of them yourself with:

```bash
zenith render examples/<name>.zen --png out.png
zenith validate examples/<name>.zen --json
```

## Shapes

| Example | Source | Output |
| ------- | ------ | ------ |
| Rectangle | [`rect.zen`](rect.zen) | ![rect](rect.png) |
| Rounded rectangle | [`rounded.zen`](rounded.zen) | ![rounded](rounded.png) |
| Ellipse | [`ellipse.zen`](ellipse.zen) | ![ellipse](ellipse.png) |
| Line | [`line.zen`](line.zen) | ![line](line.png) |
| Polygon | [`polygon.zen`](polygon.zen) | ![polygon](polygon.png) |
| Polyline | [`polyline.zen`](polyline.zen) | ![polyline](polyline.png) |
| Star | [`star.zen`](star.zen) | ![star](star.png) |
| Stroke alignment | [`stroke-align.zen`](stroke-align.zen) | ![stroke-align](stroke-align.png) |

## Fills & effects

| Example | Source | Output |
| ------- | ------ | ------ |
| Gradients (linear + radial) | [`gradient.zen`](gradient.zen) | ![gradient](gradient.png) |
| Pattern (grid + scatter) | [`pattern.zen`](pattern.zen) | ![pattern](pattern.png) |
| Drop shadow | [`shadow.zen`](shadow.zen) | ![shadow](shadow.png) |
| Blur | [`blur.zen`](blur.zen) | ![blur](blur.png) |
| Filter | [`filter.zen`](filter.zen) | ![filter](filter.png) |
| Noise / grain (filter) | [`noise.zen`](noise.zen) | ![noise](noise.png) |
| Mask | [`mask.zen`](mask.zen) | ![mask](mask.png) |

## Text

| Example | Source | Output |
| ------- | ------ | ------ |
| Hello (minimal) | [`hello.zen`](hello.zen) | ![hello](hello.png) |
| Bold | [`bold.zen`](bold.zen) | ![bold](bold.png) |
| Italic | [`italic.zen`](italic.zen) | ![italic](italic.png) |
| Rich text (inline spans) | [`richtext.zen`](richtext.zen) | ![richtext](richtext.png) |
| Decorations | [`decorations.zen`](decorations.zen) | ![decorations](decorations.png) |
| Code (syntax highlighting, dark + light) | [`code.zen`](code.zen) | ![code](code.png) |

## Layout & composition

| Example | Source | Output |
| ------- | ------ | ------ |
| Group | [`group.zen`](group.zen) | ![group](group.png) |
| Frame (clipping) | [`frame.zen`](frame.zen) | ![frame](frame.png) |
| Anchors | [`anchors.zen`](anchors.zen) | ![anchors](anchors.png) |
| Shared styles | [`styled.zen`](styled.zen) | ![styled](styled.png) |
| Image asset | [`image.zen`](image.zen) | ![image](image.png) |
| Multipage (page 1 shown) | [`multipage.zen`](multipage.zen) | ![multipage](multipage.png) |

## Diagrams & data

| Example | Source | Output |
| ------- | ------ | ------ |
| Flowchart (shapes + connectors) | [`flowchart.zen`](flowchart.zen) | ![flowchart](flowchart.png) |
| Table | [`table.zen`](table.zen) | ![table](table.png) |
