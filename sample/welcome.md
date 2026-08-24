# Quill

A text editor for macOS and Windows, written in Rust.

Everything you can see here is drawn by Quill itself: the text
buffer is our own rope, the line breaking and alignment are our
own, and each glyph is rasterised into an atlas and drawn as one
textured rectangle.

## Try these

- Select a word with the mouse, or hold shift and press an arrow key.
- Press **B**, *I*, U or S to restyle the selection.
- Pick a font family and size from the two boxes at the left of the toolbar.
- Open the opacity menu to fade the desktop through the window.
- Use the three buttons left of undo to switch between `raw`, side by side and preview.
- `File` then `Open Folder` shows any folder you like in the explorer.

## The preview

The three buttons to the left of undo switch between the raw Markdown,
the source and the preview side by side, and the preview on its own.

The parser is ours. It reads headings, **bold**, *italic*, ~~strikethrough~~,
`inline code`, fenced code blocks, lists, block quotes, rules and links, and
turns them into the same styled text Quill already knows how to lay out.

> The preview is read only. There is nothing to type into it, because what it
> shows is worked out from the source on the left.

1. Numbered lists work.
2. So do nested ones.
  - Like this.
  - And this.

```
fn main() {
    println!("code blocks keep their spacing");
}
```

See [the design document](tasks/quill-technical-design-document.md) for why it
is built the way it is.

---

Click a file in the explorer to open it. Folders open and close when you click them.
