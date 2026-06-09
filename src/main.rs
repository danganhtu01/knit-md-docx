//! `knit-md-docx` — command-line front end for the `rust_knit_md_docx` crate.

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use rust_knit_md_docx::{ConvertOptions, PageSetup};

/// Knit a Markdown file into a Word .docx with high fidelity.
#[derive(Parser, Debug)]
#[command(name = "knit-md-docx", version, about, long_about = None)]
struct Cli {
    /// Input Markdown file. Use `-` to read from standard input.
    input: PathBuf,

    /// Output .docx path. Defaults to the input path with a `.docx` extension
    /// (or `out.docx` when reading from stdin).
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Disable the GitHub Flavored Markdown extensions (tables, task lists,
    /// strikethrough, footnotes, alerts).
    #[arg(long)]
    no_gfm: bool,

    /// Enable smart (typographic) punctuation.
    #[arg(long)]
    smart: bool,

    /// Do not emit heading bookmarks for intra-document anchor links.
    #[arg(long)]
    no_anchors: bool,

    /// Render single newlines as hard line breaks instead of spaces.
    #[arg(long)]
    soft_breaks: bool,

    /// Page size.
    #[arg(long, value_enum, default_value_t = Page::A4)]
    page: Page,

    /// Body font family.
    #[arg(long)]
    body_font: Option<String>,

    /// Monospace font family used for code.
    #[arg(long)]
    code_font: Option<String>,

    /// Body font size in points.
    #[arg(long)]
    body_size: Option<f32>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Page {
    Letter,
    A4,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(out) => {
            eprintln!("Wrote {}", out.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let from_stdin = cli.input.as_os_str() == "-";

    let (markdown, base_dir, default_out) = if from_stdin {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        (s, None, PathBuf::from("out.docx"))
    } else {
        let s = std::fs::read_to_string(&cli.input)?;
        let base = cli.input.parent().map(|p| p.to_path_buf());
        let out = cli.input.with_extension("docx");
        (s, base, out)
    };

    let output = cli.output.unwrap_or(default_out);

    let mut opts = ConvertOptions {
        gfm: !cli.no_gfm,
        smart_punctuation: cli.smart,
        heading_anchors: !cli.no_anchors,
        soft_breaks_as_newlines: cli.soft_breaks,
        base_dir,
        page: match cli.page {
            Page::Letter => PageSetup::LETTER,
            Page::A4 => PageSetup::A4,
        },
        ..ConvertOptions::default()
    };
    if let Some(f) = cli.body_font {
        opts.body_font = f;
    }
    if let Some(f) = cli.code_font {
        opts.code_font = f;
    }
    if let Some(sz) = cli.body_size {
        opts.body_size_pt = sz;
    }

    rust_knit_md_docx::write_file_with(&markdown, &opts, &output)?;
    Ok(output)
}
