# Creating Quills

A Quill is a format bundle that defines how your Markdown content is rendered. This tutorial walks from an empty directory to a rendered PDF.

## 1. Create the directory

Start with this layout:

```
my-quill/
├── Quill.yaml
└── plate.typ
```

## 2. Write `Quill.yaml`

Create a minimal but complete config:

```yaml
quill:
  name: my_quill
  backend: typst
  version: "1.0.0"
  description: A simple letter format

typst:
  plate_file: plate.typ

main:
  fields:
    sender:
      type: string
      description: Name of the sender
    recipient:
      type: string
      description: Name of the recipient
    date:
      type: date       # a bare calendar date (YYYY-MM-DD); use `datetime` for wall-clock time-of-day
      description: Letter date
```

`name`, `backend`, `version`, and `description` are all required. `name` must be `snake_case`. Define your document's expected root-block fields under `main.fields`. Each field has a `type`, optional `default`, `description`, and validation constraints. Use `integer` for whole numbers only and `number` for values that may include decimals. For the full list of field types, UI hints, typed arrays, and enum constraints, see the [Quill.yaml Reference](quill-yaml-reference.md).

Use `default` for the value most authors will accept as-is (the field becomes optional, filled in when omitted). Use `example` to document the expected shape without supplying a default. Fields with neither are flagged in the blueprint with a `!must_fill` marker. See the [Quill.yaml Reference](quill-yaml-reference.md) for details.

## 3. Write `plate.typ`

Your first plate template:

```typst
#import "@local/quillmark-helper:0.1.0": data

#set page(margin: 1in)

Dear #data.recipient,

#data.at("$body", default: "")

Sincerely,

#data.sender
```

For data access patterns, helper package details, optional fields, and `$cards` iteration, see the [Typst Backend](typst-backend.md) guide.

## 4. Write a document

Create a `document.md` that matches the fields you defined:

```markdown
~~~
$quill: my_quill
$kind: main
sender: Jane Doe
recipient: John Smith
date: 2026-01-15
~~~

Thank you for your time.
```

## 5. Render it

From the same directory, render the document:

```bash
quillmark render ./my-quill document.md
```

For command options and output controls, see the [CLI Reference](../cli/reference.md).

## 6. Next steps

- [Quill.yaml Reference](quill-yaml-reference.md) — full field types, UI hints, `card_kinds`, `typst` section
- [Typst Backend](typst-backend.md) — data access patterns, `$cards` iteration, helper package
- [Quill Versioning](versioning.md)

**Tip:** To exclude files (fonts, build artifacts) from the bundle when loading from disk, add a `.quillignore` file at the bundle root using gitignore syntax.
