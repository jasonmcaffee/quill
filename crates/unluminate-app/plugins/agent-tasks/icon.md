# Making this plugin’s icon again

`icon.png` (32 by 32) and `icon-128.png` are generated on this machine rather than drawn by hand,
which is how every other bundled plugin’s icon was made — `plugins/mermaid/icon.md` records the
same recipe and `task-1795` is the ticket that asked for this one. Written down here so the icon
can be made again without guessing.

The mark is three lanes, each holding two cards. The board is what the pane is, so the mark is that
board seen from far enough away that the words on the cards have gone. Three equal lanes rather
than the uneven ones the first two renders produced: unequal columns read as a pile of blocks
rather than as a board.

## 1. Render it

Through the AI service’s `POST /image-creation/generateImageToProjectFile`, which renders with
Krea 2 and writes a verified PNG straight into this repository. It needs the local tooling token,
which every agent terminal has.

```bash
curl -s -X POST http://localhost:8091/image-creation/generateImageToProjectFile \
  -H 'Content-Type: application/json' -H "x-skip-token: $CLAUDE_SKIP_TOKEN" \
  -d '{
        "prompt": "A flat vector kanban board symbol drawn directly on a completely flat solid dark navy background filling the entire image edge to edge. Three identical vertical columns of equal width and equal height, side by side with even gaps, each column drawn as a tall rounded rectangle outline containing two small solid rounded cards stacked one above the other. The three columns together form a perfect square block, centred, with a generous even margin. Bright cyan column outlines with white and magenta cards. Bold even line weight, geometric, symmetrical, minimal, crisp, high contrast, no text, no letters, no words, no gradients, no shadows, no perspective.",
        "negativePrompt": "rounded square container, app tile, badge, outer frame, white background, light background, uneven columns, empty column, missing card, text, letters, words, numbers, watermark, signature, photorealistic, 3d render, gradient, drop shadow, noise, texture, clutter, person, face, calendar",
        "width": 1024,
        "height": 1024,
        "projectId": "unluminate",
        "relativePath": "_agent_output/task-1795-icons/agent-tasks-source.png",
        "transparentBackground": true,
        "timeoutMs": 600000
    }'
```

Two things about that call that are easy to get wrong:

- **`transparentBackground` matters.** The renderer only emits opaque pixels, so without it the
  icon is a navy square rather than a mark, and the marketplace row would draw a rectangle of the
  wrong colour behind the name.
- **The prompt has to say the background fills the image.** Asked for an “app icon” the renderer
  draws a rounded tile on white, and the flood fill then keys the *white* out and keeps the tile —
  a dark square where the mark should be.

## 2. Turn it into the two icons

```bash
cargo run --example plugin_icon -- _agent_output/task-1795-icons/agent-tasks-source.png crates/unluminate-app/plugins/agent-tasks
```

`examples/plugin_icon.rs` keys the flat background out by flood filling from the four corners,
crops to the mark with an even margin, squares it, and scales it to 128 and to 32 with a smooth
filter.

## 3. Look at it

```bash
cargo run --example scale -- crates/unluminate-app/plugins/agent-tasks/icon.png /tmp/icon-large.png 8
```

The 32 by 32 one is the one to check, because it is the size the marketplace row uses and it is
where a mark with too much detail in it turns to mush.
