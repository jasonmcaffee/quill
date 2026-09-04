# Making this plugin's icon again

`icon.png` (32 by 32) and `icon-128.png` are generated on this machine rather than drawn by hand,
which is how the other bundled plugins' icons were made — `plugins/mermaid/icon.md` and
`tasks/unluminous-ide-tdd.md` §6 record the same recipe. Written down here so the icon can be made again
without guessing.

The mark is a paint brush over one round swatch of colour. Two shapes and two colours: a stylesheet
is about what things look like, and a brush is the shortest way to say that at 32 by 32, which is the
size an explorer row and a tab draw it at.

## 1. Render it

Through the AI service's `POST /image-creation/generateImageToProjectFile`, which renders with Krea 2
and writes a verified PNG straight into this repository. It needs the local tooling token, which
every agent terminal has.

```bash
curl -s -X POST http://localhost:8091/image-creation/generateImageToProjectFile \
  -H 'Content-Type: application/json' -H "x-skip-token: $CLAUDE_SKIP_TOKEN" \
  -d '{
    "prompt": "A flat vector app icon: one bold paint brush standing upright at a slight diagonal, with a wide flat cyan head and a simple straight handle, over a single large rounded square colour swatch behind it. Two solid colours only plus the background. Bold even shapes, geometric, perfectly centred, generous margin, no outlines around the shapes. Bright cyan brush, magenta swatch, on a completely flat solid dark navy background. Minimal, crisp, high contrast, icon design, no text, no letters, no words, no gradients, no shadows, no perspective, no white outline.",
    "negativePrompt": "text, letters, words, numbers, watermark, signature, photorealistic, 3d render, gradient, drop shadow, noise, texture, busy background, clutter, person, face, hand, paint splatter, realistic bristles, white outline, stroke, border",
    "width": 1024, "height": 1024,
    "projectId": "unluminous",
    "relativePath": "_agent_output/task-1671-css-plugin/icon-source.png",
    "transparentBackground": true,
    "timeoutMs": 600000
  }'
```

Two things about that call that are easy to get wrong:

- **`transparentBackground` matters.** The renderer only emits opaque pixels, so without it the icon
  is a navy square rather than a mark, and an explorer row would show a rectangle of the wrong colour
  behind every `.css` file.
- **The prompt and the negative prompt both refuse a white outline.** The first attempt asked for a
  brush across three swatches and came back with every shape ringed in white; at 32 by 32 the rings
  are most of the picture and the mark turns to mush. Fewer shapes, no outline.

## 2. Turn it into the two icons

```bash
cargo run --example plugin_icon -- _agent_output/task-1671-css-plugin/icon-source.png crates/unluminous-app/plugins/css
```

`examples/plugin_icon.rs` keys the flat background out by flood filling from the four corners, crops
to the mark with an even margin, squares it, and scales it to 128 and to 32 with a smooth filter. The
folder it writes into has to exist already.

## 3. Look at it

```bash
cargo run --example scale -- crates/unluminous-app/plugins/css/icon.png _agent_output/task-1671-css-plugin/icon-large.png 8
```

The 32 by 32 one is the one to check, because it is the size a tab and an explorer row use and it is
where a mark with too much detail in it turns to mush.
