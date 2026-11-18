<h1 align="center">pdftk-rs</h1>

<p align="center">
  <b>pdftk, without the Java.</b><br>
  A Rust port of the PDF toolkit: one small static binary, same command line.
</p>

<p align="center">
  <a href="https://github.com/nickypro/pdftk-rs/actions/workflows/ci.yml"><img src="https://github.com/nickypro/pdftk-rs/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/nickypro/pdftk-rs/releases/latest"><img src="https://img.shields.io/github/v/release/nickypro/pdftk-rs?sort=semver" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--2.0--or--later-blue" alt="License: GPL-2.0-or-later"></a>
</p>

---

`brew install pdftk-java` pulls in a 220 MB Java runtime to run a 6 MB tool.
`pdftk-rs` is the same tool as a single ~3 MB executable with no runtime
dependencies. It is a port of [pdftk-java](https://gitlab.com/pdftk-java/pdftk)
3.3.3: the same operations, the same argument grammar, the same messages and
exit codes, so existing scripts keep working.

```console
$ pdftk-rs A=report.pdf B=appendix.pdf cat A1-3 B A4-end output merged.pdf
$ pdftk-rs scan.pdf rotate 1-endeast output upright.pdf
$ pdftk-rs form.pdf fill_form answers.fdf output filled.pdf flatten
$ pdftk-rs secret.pdf output locked.pdf owner_pw PROMPT allow printing
```

## Install

**Homebrew** (macOS and Linux)

```sh
brew install nickypro/tap/pdftk-rs
```

**Prebuilt binaries** for macOS, Linux and Windows (x86-64 and arm64) are on
the [releases page](https://github.com/nickypro/pdftk-rs/releases/latest).
Unpack and put `pdftk-rs` on your `PATH`.

**From source**

```sh
cargo install --locked --git https://github.com/nickypro/pdftk-rs
```

If you want it to answer to the old name, add `alias pdftk=pdftk-rs`.

## What it does

Everything pdftk does. `pdftk-rs --help` prints the full manual.

| Operation | |
|---|---|
| `cat`, `shuffle` | merge, split, reorder and interleave pages, with ranges like `1-3`, `r2-end`, `2-endeven`, `1-endeast` |
| `burst` | split into one file per page |
| `rotate` | rotate pages in place |
| `stamp`, `background`, `multistamp`, `multibackground` | overlay or underlay another PDF |
| `fill_form`, `flatten`, `generate_fdf`, `dump_data_fields` | fill forms from FDF or XFDF, flatten them, export field data |
| `dump_data`, `update_info`, `dump_data_annots` | read and write metadata, bookmarks, page labels |
| `attach_files`, `unpack_files` | embed and extract attachments |
| `compress`, `uncompress` | pack or unpack page streams for hand editing |
| `owner_pw`, `user_pw`, `allow`, `input_pw` | encrypt (RC4 40/128, AES-128) and decrypt, with permissions |

It also opens some files the Java tool refuses: AES-256 encrypted PDFs, and
several kinds of damaged file.

## How it is tested

The Java `pdftk` is the specification. Most of the 139 tests run both programs
on the same command line and compare the results:

- exit code, stdout and stderr, byte for byte;
- for PDF output: `qpdf --check`, page count, per-page extracted text, the
  Java tool's own `dump_data` of both files, and for stamps a raster comparison;
- upstream's own test cases (`CatTest`, `FormTest`, `CryptoTest`, …) are ported
  along with their fixtures.

[CI](https://github.com/nickypro/pdftk-rs/actions/workflows/ci.yml) runs that
suite against pdftk-java on every push, and builds and smoke-tests the binary on
Linux, macOS and Windows.

## Verifying a download

Release binaries are built by GitHub Actions from the tagged commit, using only
GitHub's own actions. Each archive has a SHA-256 in `SHA256SUMS` and a signed
[build provenance attestation](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations),
which you can check with the GitHub CLI:

```sh
gh attestation verify pdftk-rs-v0.1.0-aarch64-apple-darwin.tar.gz --repo nickypro/pdftk-rs
```

## Differences from pdftk-java

Output PDFs are equivalent, not byte-identical: object numbering, the page
tree layout and stream compression differ. Beyond that:

- **`replacement_font` is not supported.** It prints a warning and fills with
  the form's own fonts. CJK fonts and right-to-left shaping in generated form
  appearances are not ported either.
- **Real numbers are stored as 32-bit floats**, so a box of `595.303937` is
  written back as `595.30396`.
- **Encrypted inputs** with no trailer `/ID`, or with a direct `/Encrypt`
  dictionary, cannot be opened.
- **Producer** in `cat`/`shuffle`/`burst` output is `pdftk-rs`, not iText.
- Where pdftk-java crashes with a stack trace on malformed input, `pdftk-rs`
  reports an error or carries on.
- Answering "y" to an overwrite prompt under `do_ask` overwrites the file.

This is a young port. If it disagrees with pdftk-java on something not listed
here, that is a bug: please [open an issue](https://github.com/nickypro/pdftk-rs/issues)
with the command line and, if you can, the PDF.

## Versions

Releases follow [semantic versioning](https://semver.org). The version is set in
`Cargo.toml`; pushing a matching `vX.Y.Z` tag builds and publishes the release.
`pdftk-rs --version` reports it, along with the pdftk-java version it tracks.

## Credits and license

pdftk-rs is a derivative work of [pdftk-java](https://gitlab.com/pdftk-java/pdftk)
by Marc Vinyals, itself a port of [PDFtk](https://www.pdflabs.com/tools/pdftk-the-pdf-toolkit/)
by Sid Steward (Steward and Lee, LLC), and is licensed the same way:
**GPL-2.0-or-later** (see [LICENSE](LICENSE)). The PDF object layer is
[lopdf](https://github.com/J-F-Liu/lopdf). `resources/` holds data files taken
from pdftk-java and its bundled iText fork (help text, the standard-14 font
metrics, the glyph list); their notices are in `resources/license_gpl_pdftk/`.

The port was written with [Claude](https://claude.com/claude-code) and checked
against the Java implementation as described above.
