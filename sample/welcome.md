# Quill

A text editor for macOS and Windows, written in Rust.

Everything you can see here is drawn by Quill itself: the text
buffer is our own rope, the line breaking and alignment are our
own, and each glyph is rasterised into an atlas and drawn as one
textured rectangle.

## Try these

- Select a word with the mouse, or hold shift and press an arrow key.
- Press **B**, *I*, U or S to restyle the selection.
- `Edit` then `Settings` opens the font and the background opacity.
- The three buttons at the right of the toolbar switch between `raw`, side by side and preview.
- Drag the edge of the explorer to make it wider, and double click the edge to put it back.
- Control and backtick opens a terminal along the bottom, with a tab for each shell.
- `File` then `New Window` opens a second Quill, and `Recent Projects` lists the folders you have had open.

## The preview

The three buttons at the right of the toolbar switch between the raw Markdown,
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

## The files

Any file holding text opens, whether Quill knows the kind of file or not, so
`example.rs` in the `notes` folder opens as plain text. A file that is not text,
such as an image, is listed and dimmed and says why when you point at it.

---

Click a file in the explorer to open it. Folders open and close when you click them.
