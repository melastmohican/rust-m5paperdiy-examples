# Books used by the tests

Two unmodified e-books from Project Gutenberg, kept under their own names here so the tests run on real
books in CI without a download:

| File | Book | Gutenberg |
| :--- | :--- | :--- |
| `journey.epub` | *A Journey to the Centre of the Earth*, Jules Verne | [#18857](https://www.gutenberg.org/ebooks/18857) |
| `gulliver.epub` | *Gulliver's Travels into Several Remote Nations of the World*, Jonathan Swift | [#829](https://www.gutenberg.org/ebooks/829) |

Both texts are in the public domain in the USA (`dc:rights` of each package) and their authors died in 1905 and
1745. The files carry the Project Gutenberg license text inside them, as that license asks of anyone who
redistributes them, and are not changed. The GPL-3.0-only license of this repository does not apply to them.

They have the shapes the reader has to handle: `journey.epub` has 91 contents entries in 6 documents, told
apart by `#fragment` anchors, and 4 PNG pictures; `gulliver.epub` has one document for each chapter and a
227 KB JPEG (the word frame of Lagado, in the chapter "III.V").
