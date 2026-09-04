# Making this plugin's icon again

`icon.png` (32 by 32) and `icon-128.png` are generated on this machine rather than drawn by hand,
which is how the other three bundled plugins' icons were made — `tasks/unluminous-ide-tdd.md` §6 records
the same recipe. Written down here so the icon can be made again without guessing.

The mark is a small flow diagram — one node branching into two — because that is what a Mermaid file
holds more often than anything else, and because it reads at 32 by 32, which a mermaid does not.

## 1. Render it

Through the AI service's `POST /image-creation/generateImageToProjectFile`, which renders with
Krea 2 and writes a verified PNG straight into this repository. It needs the local tooling token,
which every agent terminal has.

```bash
curl -s -X POST http://localhost:8091/image-creation/generateImageToProjectFile \
  -H 'Content-Type: application/json' -H "x-skip-token: $CLAUDE_SKIP_TOKEN" \
  -d '{
    "prompt": "A flat vector app icon of a simple flow diagram: three rounded rectangle nodes joined by two clean arrows, arranged as one node at the top and two below it, forming a small branching chart. Bold even line weight, geometric, perfectly centred, generous margin. Bright cyan and magenta nodes with white connecting arrows, on a completely flat solid dark navy background. Minimal, crisp, high contrast, icon design, no text, no letters, no words, no gradients, no shadows, no perspective.",
    "negativePrompt": "text, letters, words, numbers, watermark, signature, photorealistic, 3d render, gradient, drop shadow, noise, texture, busy background, clutter, mermaid, person, face, fish",
    "width": 1024, "height": 1024,
    "projectId": "unluminous",
    "relativePath": "_agent_output/task-1660-mermaid/icon-source.png",
    "transparentBackground": true,
    "timeoutMs": 600000
  }'
```

Two things about that call that are easy to get wrong:

- **`transparentBackground` matters.** The renderer only emits opaque pixels, so without it the icon
  is a navy square rather than a mark, and an explorer row would show a rectangle of the wrong colour
  behind every `.mmd` file.
- **The negative prompt says `mermaid`.** Asking for a "Mermaid diagram" icon otherwise produces a
  picture of a mermaid, which is a fish and not a flowchart.

## 2. Turn it into the two icons

```bash
cargo run --example plugin_icon -- _agent_output/task-1660-mermaid/icon-source.png crates/unluminous-app/plugins/mermaid
```

`examples/plugin_icon.rs` keys the flat background out by flood filling from the four corners, crops
to the mark with an even margin, squares it, and scales it to 128 and to 32 with a smooth filter.
Flood filling from the corners rather than matching a colour everywhere is deliberate: a node in the
mark may be near the background's own colour, and matching everywhere would eat it.

## 3. Look at it

```bash
cargo run --example scale -- crates/unluminous-app/plugins/mermaid/icon.png /tmp/icon-large.png 8
```

The 32 by 32 one is the one to check, because it is the size a tab and an explorer row use and it is
where a mark with too much detail in it turns to mush.
