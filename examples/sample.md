---
title: Full Fidelity Sample
author: knit-md-docx
---

# Knit Markdown to DOCX

This document exercises the full feature set. Here is a paragraph with **bold**,
*italic*, ***bold italic***, ~~strikethrough~~, `inline code`, and a
soft-wrapped
line that should join with a space.

## Inline elements

A [normal external link](https://example.com), an autolink <https://rust-lang.org>,
an email autolink <hello@example.com>, and an [internal link](#lists) to a later
heading. Inline math like $E = mc^2$ becomes a native Word equation.

Hard break below:
first line\
second line.

### Emphasis nesting

You can **mix _emphasis_ and `code`** inside the same run, and even
**bold across *italic* boundaries**.

## Lists

### Unordered

- First bullet
- Second bullet
  - Nested bullet
  - Another nested
    - Deeper still
- Back to top level

### Ordered

1. First
2. Second
   1. Nested one
   2. Nested two
3. Third

### Ordered with a custom start

5. Five
6. Six
7. Seven

### Task list

- [x] Completed task
- [ ] Pending task
- [x] Another done item

## Block quotes

> A simple block quote.
>
> > A nested block quote inside it.

> [!NOTE]
> This is a GitHub-style alert.

> [!WARNING]
> Be careful here.

## Code

Inline `let x = 42;` then a fenced block:

```rust
fn main() {
    // indentation should be preserved
    let greeting = "hello, world";
    println!("{greeting}");
}
```

## Tables

| Feature      | Supported | Notes                |
|:-------------|:---------:|---------------------:|
| Headings     |    Yes    |     six levels       |
| Tables       |    Yes    |  with alignment      |
| Footnotes    |    Yes    |   real Word notes    |

## Footnotes

Here is a statement that needs a citation.[^src] And another one.[^second]

[^src]: The first footnote body, with *emphasis*.
[^second]: A second footnote referencing <https://example.com>.

## Definition list

Term one
: Definition of term one.

Term two
: Definition of term two.

## Thematic break

Above the line.

---

Below the line.

## HTML

Some <b>bold via HTML</b> and a <mark>highlighted</mark> span, plus a hard<br>break.
Vertical alignment: x<sup>2</sup> and a<sub>n</sub> via HTML tags.

## Superscript & subscript

Markdown shorthand: water is H~2~O, carbon dioxide is CO~2~, and the 1^st^ and
2^nd^ items are ordinals. Glucose is C~6~H~12~O~6~.

## Math

Inline: the quadratic formula $x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$, a sum
$\sum_{i=1}^{n} i = \frac{n(n+1)}{2}$, and a set relation
$\forall \varepsilon > 0, \exists \delta > 0$.

Greek and operators: $\alpha\beta\gamma$, $\theta \leq \pi$, $a \cdot b \neq 0$.

Display equations:

$$
\int_0^\infty e^{-x}\,dx = 1
$$

$$
e^{i\pi} + 1 = 0
$$

$$
\left( \sum_{k=1}^{n} a_k \right)^2 \leq n \sum_{k=1}^{n} a_k^2
$$

The end.
