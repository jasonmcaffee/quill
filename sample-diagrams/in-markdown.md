# Diagrams inside a document

A fence whose language is mermaid is drawn rather than shown as code, which is the other half of what
`task-1660` asks for. Everything round it is ordinary Markdown: **bold**, *italic*,
`inline code`, and a list.

- The fence is gathered whole rather than a line at a time.
- Its paragraph is left empty and the window paints into the room it reserved.
- A fence nobody has closed yet still draws what it has.

```mermaid
flowchart LR
  Source[The source] --> Read[Read it]
  Read --> Layout[Lay it out]
  Layout --> Scene[The scene]
  Scene --> Paint[Paint it]
```

Two diagrams in one document are both found, in the order they appear.

```mermaid
pie showData title Two halves of the ticket
  "A .mmd file" : 1
  "A fence in Markdown" : 1
```

And a fence that is **not** mermaid is still shown as code, which it always was:

```rust
fn main() {
    println!("still code");
}
```

A diagram that will not parse keeps its room and says which line went wrong, rather than taking the
rest of the document away with it:

```mermaid
flowchart LR
  A --> B
  C[never closed --> D
```

That is the last of it.
