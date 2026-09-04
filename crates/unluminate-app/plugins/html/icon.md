# Making this plugin's icon again

`icon.png` (32 by 32) and `icon-128.png` are generated on this machine rather than drawn by hand,
which is how the other bundled plugins' icons were made. Written down here so the icon can be made
again without guessing.

The mark is a clean HTML tag glyph, `< / >`, in HTML5 orange on a flat dark background. The angle
brackets and slash make the file type readable at 32 by 32, the size a tab and an explorer row draw.

## 1. Render it

Through the AI service's `POST /image-creation/generateImageToProjectFile`, which renders with Krea 2
and writes a verified PNG into this repository's task output folder. It needs the local tooling token,
which every agent terminal has.

```bash
curl -s -X POST http://localhost:8091/image-creation/generateImageToProjectFile \
  -H 'Content-Type: application/json' -H "x-skip-token: $CLAUDE_SKIP_TOKEN" \
  -d '{
    "prompt": "A flat vector app icon featuring a bold HTML tag glyph made from clean angle brackets and a slash, < / >, in vivid HTML orange, centred inside a rounded square tile. Geometric even shapes, generous margin, crisp high contrast, minimal developer-tool icon, no text beyond the abstract tag glyph, on a completely flat solid dark navy background, no gradients, shadows, perspective, or texture.",
    "negativePrompt": "watermark, signature, photorealistic, 3d render, gradient, drop shadow, noise, texture, busy background, clutter, person, face, hand, realistic object, white outline, border, extra letters, words, numbers",
    "width": 1024, "height": 1024,
    "projectId": "unluminate",
    "relativePath": "_agent_output/task-1694-html-plugin/icon-source.png",
    "transparentBackground": true,
    "timeoutMs": 600000
  }'
```

`transparentBackground` matters: the renderer emits an opaque flat background, and the icon tool
flood-fills that background to transparency before cropping and scaling the mark.

## 2. Turn it into the two icons

```bash
cargo run --example plugin_icon -- _agent_output/task-1694-html-plugin/icon-source.png crates/unluminate-app/plugins/html
```

`examples/plugin_icon.rs` keys the flat background out by flood filling from the four corners, crops
to the mark with an even margin, and scales it to 128 and to 32 with a smooth filter. The 32 by 32
one is the size to check, because that is where a mark with too much detail turns to mush.
