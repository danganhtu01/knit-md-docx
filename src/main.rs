//! `knit-md-docx` — command-line front end for the `knit_md_docx` crate.

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use knit_md_docx::{ConvertOptions, PageSetup, Theme};

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

    /// TOML theme file setting any fonts/sizes/colours/toggles. Individual
    /// flags below override values from this file.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

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

    /// Page size [default: a4].
    #[arg(long, value_enum)]
    page: Option<Page>,

    // -- Fonts ---------------------------------------------------------------
    /// Body font family.
    #[arg(long)]
    body_font: Option<String>,

    /// Heading font family.
    #[arg(long)]
    heading_font: Option<String>,

    /// Monospace font family used for code.
    #[arg(long)]
    code_font: Option<String>,

    // -- Sizes (points) ------------------------------------------------------
    /// Body font size in points.
    #[arg(long)]
    body_size: Option<f32>,

    /// Code (monospace) font size in points.
    #[arg(long)]
    code_size: Option<f32>,

    /// Caption / muted-text font size in points.
    #[arg(long)]
    caption_size: Option<f32>,

    /// Multiply every heading size by this factor (applied after other sizing).
    #[arg(long)]
    heading_scale: Option<f32>,

    // -- Colours (6-digit hex, `#` optional) ---------------------------------
    /// Heading accent colour.
    #[arg(long, value_name = "HEX")]
    heading_color: Option<String>,

    /// Hyperlink colour.
    #[arg(long, value_name = "HEX")]
    link_color: Option<String>,

    /// Caption / muted-text colour.
    #[arg(long, value_name = "HEX")]
    caption_color: Option<String>,

    /// Block-quote text colour.
    #[arg(long, value_name = "HEX")]
    quote_color: Option<String>,

    /// Fenced/indented code-block shading fill.
    #[arg(long, value_name = "HEX")]
    code_fill: Option<String>,
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

    // Precedence: defaults < theme file (--config) < individual CLI flags.
    // The binary's baseline page is A4 (the library default is Letter); a theme
    // file or --page can still override it.
    let mut opts = ConvertOptions {
        page: PageSetup::A4,
        base_dir,
        ..ConvertOptions::default()
    };

    if let Some(path) = &cli.config {
        Theme::from_toml_file(path)?.apply(&mut opts)?;
    }

    // Style/font/colour/size flags share the theme machinery (and its hex
    // validation), so build a one-off Theme from the flags and overlay it.
    let cli_theme = Theme {
        body_font: cli.body_font,
        heading_font: cli.heading_font,
        code_font: cli.code_font,
        body_size: cli.body_size,
        code_size: cli.code_size,
        caption_size: cli.caption_size,
        heading_color: cli.heading_color,
        link_color: cli.link_color,
        caption_color: cli.caption_color,
        quote_color: cli.quote_color,
        code_fill: cli.code_fill,
        page: cli.page.map(|p| match p {
            Page::Letter => "letter".to_string(),
            Page::A4 => "a4".to_string(),
        }),
        ..Theme::default()
    };
    cli_theme.apply(&mut opts)?;

    // Heading scale is applied last so it multiplies whatever sizes resulted.
    if let Some(scale) = cli.heading_scale {
        for s in &mut opts.heading_sizes_pt {
            *s *= scale;
        }
    }

    // Presence-only toggles force their (one-directional) effect when given.
    if cli.no_gfm {
        opts.gfm = false;
    }
    if cli.smart {
        opts.smart_punctuation = true;
    }
    if cli.no_anchors {
        opts.heading_anchors = false;
    }
    if cli.soft_breaks {
        opts.soft_breaks_as_newlines = true;
    }

    knit_md_docx::write_file_with(&markdown, &opts, &output)?;
    Ok(output)
}
