# Unluminate

A text editor for macOS and Windows, written in Rust.

Everything you can see here is drawn by Unluminate itself: the text
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
- `File` then `New Window` opens a second Unluminate, and `Recent Projects` lists the folders you have had open.

## The preview

The three buttons at the right of the toolbar switch between the raw Markdown,
the source and the preview side by side, and the preview on its own.

The parser is ours, and it reads what CommonMark and GitHub Flavored Markdown
describe: headings, **bold**, *italic*, ~~strikethrough~~, `inline code`, fenced
and indented code blocks, tight and loose lists, task lists, nested block quotes,
tables, rules, links, reference links, footnotes, autolinks and escapes. All of
it becomes the same styled text Unluminate already knows how to lay out, which is why
you can select a passage of the preview with the mouse and copy it.

> The preview is read only. There is nothing to type into it, because what it
> shows is worked out from the source on the left.

1. Numbered lists work.
2. So do nested ones.
   - Like this.
   - And this.

A task list has real tick boxes:

- [x] This one is ticked.
- [ ] This one is not.

A fence that names a language is coloured by the plugin that reads it, on a
panel of its own:

```rust
fn main() {
    let greeting = "code blocks keep their spacing";
    println!("{greeting}");
}
```

And a table is a table:

| Crate | What is in it | Tests |
| -------------- | ------------------------------- | ----: |
| unluminate-core | the editor, with no window | 639 |
| unluminate-terminal | the pseudoterminal and a screen | 116 |
| unluminate-app | drawing, fonts, settings, menus | 796 |

## The files

Any file holding text opens, whether Unluminate knows the kind of file or not, so
`example.rs` in the `notes` folder opens as plain text. A file that is not text,
such as an image, is listed and dimmed and says why when you point at it.

---

Click a file in the explorer to open it. Folders open and close when you click them.
