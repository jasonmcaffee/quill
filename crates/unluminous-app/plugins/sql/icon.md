# Making this plugin's icon again

`icon.png` (32 by 32) and `icon-128.png` are generated on this machine rather than drawn by hand,
which is how the other bundled plugins' icons were made — `plugins/css/icon.md` and
`plugins/mermaid/icon.md` record the same recipe. Written down here so the icon can be made again
without guessing.

The mark is the cylinder every tool in the world draws for a database: three stacked bands with an
elliptical top. Two colours and one shape, because at 32 by 32 — which is what an explorer row and a
tab draw it at — anything else turns to mush. The middle band is the second colour, so the icon reads
as *stacked* rather than as a drum.

## 1. Render it

Through the AI service's `POST /image-creation/generateImageToProjectFile`, which renders with Krea 2
and writes a verified PNG straight into the repository. It needs the local tooling token, which every
agent terminal has.

```bash
curl -s -X POST http://localhost:8091/image-creation/generateImageToProjectFile \
  -H 'Content-Type: application/json' -H "x-skip-token: $CLAUDE_SKIP_TOKEN" \
  -d '{
    "prompt": "A flat vector app icon: one bold database cylinder made of three stacked horizontal bands with clean elliptical tops, standing centred. Two solid colours only plus the background. Bold even shapes, geometric, perfectly centred, generous margin, no outlines around the shapes. Bright cyan cylinder with one magenta band, on a completely flat solid dark navy background. Minimal, crisp, high contrast, icon design, no text, no letters, no words, no gradients, no shadows, no perspective, no white outline.",
    "negativePrompt": "text, letters, words, numbers, watermark, signature, photorealistic, 3d render, gradient, drop shadow, noise, texture, busy background, clutter, person, face, hand, server rack, cable, realistic metal, white outline, stroke, border",
    "width": 1024, "height": 1024,
    "projectId": "unluminous",
    "relativePath": "_agent_output/task-1777-database-plugin/icon-source.png",
    "transparentBackground": true,
    "timeoutMs": 600000
  }'
```

`transparentBackground` matters: the renderer only emits opaque pixels, so without it the icon is a
navy square rather than a mark, and an explorer row would show a rectangle of the wrong colour behind
every `.sql` file. And both prompts refuse a white outline, which is the fault
`plugins/css/icon.md` records — at 32 by 32 the rings are most of the picture.

**It writes into the project called `unluminous`, which is the main checkout**, wherever this branch is
being worked in. Copy the source picture into the checkout you are working in before step 2, and
delete the one it wrote.

## 2. Turn it into the two icons

```bash
cargo run --example plugin_icon -- _agent_output/task-1777-database-plugin/icon-source.png crates/unluminous-app/plugins/sql
```

`examples/plugin_icon.rs` keys the flat background out by flood filling from the four corners, crops
to the mark with an even margin, squares it, and scales it to 128 and to 32 with a smooth filter. The
folder it writes into has to exist already.

## 3. Look at it

```bash
cargo run --example scale -- crates/unluminous-app/plugins/sql/icon.png _agent_output/task-1777-database-plugin/icon-large.png 8
```

The 32 by 32 one is the one to check, because it is the size a tab and an explorer row use.

## The Database plugin has no icon of its own, and does not need one

`plugins/database/plugin.conf` is a `ui` plugin: it claims no file type, so there is no explorer row
and no tab to put a picture in front of. Its rail button is `pane.icon = database`, a **drawn** icon
from `theme::icon`, which takes the tint the rail gives it and follows the window's own three states —
which a picture could not. The same is true of Agent-Tasks and Agent-Chat, and it is why
`the_surfaces_are_what_the_enabled_plugins_contribute_and_nothing_else` asserts that a `ui` plugin
ships neither an icon nor a colour scheme.
