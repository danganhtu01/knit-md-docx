//! A small LaTeX-subset → OMML (`OMath`) converter.
//!
//! Word stores equations as a structured OMML tree (`m:oMath`), not as linear
//! text, so to get *native, editable* equations we parse a useful subset of
//! LaTeX math and build a [`docx_rs::OMath`]. The subset covers the constructs
//! that appear in the overwhelming majority of Markdown math:
//!
//! - superscripts `^` and subscripts `_` (with `{...}` groups or single tokens),
//! - fractions `\frac{a}{b}`,
//! - radicals `\sqrt{x}` and `\sqrt[n]{x}`,
//! - n-ary operators `\sum` / `\prod` / `\int` (and friends) with `_`/`^` limits,
//! - delimiters `\left( ... \right)`,
//! - Greek letters and a broad table of math symbols/operators.
//!
//! Anything unrecognised degrades gracefully: an unknown `\command` is emitted
//! as literal text, so no input is ever lost. The result is a real Word equation
//! object that opens in the equation editor — a strict upgrade over rendering the
//! LaTeX source as italic text.

use docx_rs::{OMath, OMathElement};

/// Parse a LaTeX math string into a native [`OMath`]. `display` selects a block
/// (centred, `m:oMathPara`) equation versus an inline one.
pub(crate) fn latex_to_omath(src: &str, display: bool) -> OMath {
    let mut p = Parser::new(src);
    let mut elements = p.parse_sequence(None, false);
    if elements.is_empty() {
        // Never emit a truly empty equation (Word shows an empty box).
        elements.push(OMathElement::Run(src.to_string()));
    }
    let math = OMath::from_elements(elements);
    if display { math.display() } else { math }
}

/// Maximum nesting depth before the parser stops descending and emits the
/// remainder as literal text. Real equations never approach this; the cap exists
/// so adversarial input (e.g. tens of thousands of nested braces) degrades
/// gracefully instead of overflowing the stack.
const MAX_DEPTH: usize = 128;

struct Parser {
    chars: Vec<char>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn new(src: &str) -> Self {
        Parser {
            chars: src.chars().collect(),
            pos: 0,
            depth: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    /// Consume `expected` only if it is the current char. Guards against an early
    /// sequence break (e.g. a stray `\right`) leaving the cursor on the wrong
    /// token, where an unconditional bump would swallow it.
    fn eat(&mut self, expected: char) {
        if self.peek() == Some(expected) {
            self.bump();
        }
    }

    /// Parse elements until `stop` (a closing `}`/`]` etc.) or end of input.
    /// The stop character is left unconsumed for the caller to handle. `in_delim`
    /// is true only inside a `\left … \right` body, so a stray `\right` elsewhere
    /// is treated as literal text rather than a spurious terminator.
    fn parse_sequence(&mut self, stop: Option<char>, in_delim: bool) -> Vec<OMathElement> {
        // Depth guard: refuse to recurse past the cap. Consume the rest of the
        // input as a literal run so output stays well-formed and bounded.
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            let rest: String = self.chars[self.pos..].iter().collect();
            self.pos = self.chars.len();
            return if rest.is_empty() {
                Vec::new()
            } else {
                vec![OMathElement::Run(rest)]
            };
        }

        let mut out: Vec<OMathElement> = Vec::new();
        loop {
            match self.peek() {
                None => break,
                Some(c) if Some(c) == stop => break,
                // `\right` ends a `\left ... \right` group; leave it for the caller.
                Some('\\') if in_delim && self.looks_at_command("right") => break,
                _ => {}
            }
            // Skip plain whitespace (math layout ignores it).
            if matches!(self.peek(), Some(' ') | Some('\t') | Some('\n')) {
                self.bump();
                continue;
            }
            let nodes = self.parse_scripted_atom();
            if nodes.is_empty() {
                continue;
            }
            out.extend(nodes);
        }
        self.depth -= 1;
        merge_runs(out)
    }

    /// Parse a single atom together with any trailing `^`/`_` scripts. An
    /// unscripted multi-node atom is spliced in directly rather than wrapped.
    fn parse_scripted_atom(&mut self) -> Vec<OMathElement> {
        let base = self.parse_atom();
        if base.is_empty() {
            return Vec::new();
        }
        self.attach_scripts(base)
    }

    /// After parsing a base atom, consume any trailing `^`/`_` scripts and wrap
    /// the base accordingly (handles `x^2`, `x_i`, `x_i^2`, `x^2_i`). With no
    /// scripts the base nodes are returned unchanged.
    fn attach_scripts(&mut self, base: Vec<OMathElement>) -> Vec<OMathElement> {
        let mut sub: Option<Vec<OMathElement>> = None;
        let mut sup: Option<Vec<OMathElement>> = None;

        loop {
            self.skip_spaces();
            match self.peek() {
                Some('^') if sup.is_none() => {
                    self.bump();
                    sup = Some(self.parse_script_arg());
                }
                Some('_') if sub.is_none() => {
                    self.bump();
                    sub = Some(self.parse_script_arg());
                }
                _ => break,
            }
        }

        match (sub, sup) {
            (None, None) => base,
            (None, Some(sup)) => vec![OMathElement::SuperScript { base, sup }],
            (Some(sub), None) => vec![OMathElement::SubScript { base, sub }],
            (Some(sub), Some(sup)) => vec![OMathElement::SubSuperScript { base, sub, sup }],
        }
    }

    /// The argument of a `^`/`_`: a `{group}` or a single token.
    fn parse_script_arg(&mut self) -> Vec<OMathElement> {
        self.skip_spaces();
        match self.peek() {
            Some('{') => {
                self.bump();
                let inner = self.parse_sequence(Some('}'), false);
                self.eat('}');
                inner
            }
            _ => self.parse_atom(),
        }
    }

    /// Parse one "atom": a group, a command, or a single literal char. Returns a
    /// sequence because a `{...}` group or some commands expand to several nodes.
    fn parse_atom(&mut self) -> Vec<OMathElement> {
        match self.peek() {
            Some('{') => {
                self.bump();
                let inner = self.parse_sequence(Some('}'), false);
                self.eat('}');
                inner
            }
            Some('\\') => self.parse_command(),
            Some(c) => {
                self.bump();
                vec![OMathElement::Run(c.to_string())]
            }
            None => Vec::new(),
        }
    }

    /// Parse a `\command` and its arguments.
    fn parse_command(&mut self) -> Vec<OMathElement> {
        self.bump(); // consume '\'

        // Control symbols: a backslash followed by a single non-letter, e.g.
        // `\{`, `\,`, `\%`. These are taken as one char.
        if let Some(c) = self.peek() {
            if !c.is_alphabetic() {
                self.bump();
                if let Some(s) = control_symbol(c) {
                    return if s.is_empty() {
                        Vec::new()
                    } else {
                        vec![OMathElement::Run(s.to_string())]
                    };
                }
                return vec![OMathElement::Run(c.to_string())];
            }
        }

        let name = self.read_command_name();
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" => {
                let numerator = self.parse_group_arg();
                let denominator = self.parse_group_arg();
                vec![OMathElement::Fraction {
                    numerator,
                    denominator,
                }]
            }
            "sqrt" => {
                self.skip_spaces();
                let degree = if self.peek() == Some('[') {
                    self.bump();
                    let deg = self.parse_sequence(Some(']'), false);
                    self.eat(']');
                    Some(deg)
                } else {
                    None
                };
                let radicand = self.parse_group_arg();
                vec![OMathElement::Radical { degree, radicand }]
            }
            "left" => self.parse_delimited(),
            cmd if nary_operator(cmd).is_some() => {
                let op = nary_operator(cmd).unwrap().to_string();
                self.parse_nary(op)
            }
            // Known symbol/Greek/function: emit its glyph (or upright name).
            other => match command_text(other) {
                Some("") => Vec::new(),
                Some(text) => vec![OMathElement::Run(text.to_string())],
                // Unknown command: keep the raw name so nothing is silently lost.
                None => vec![OMathElement::Run(other.to_string())],
            },
        }
    }

    /// `\left( ... \right)` → a delimited group. Falls back to literal text if
    /// the structure is malformed.
    fn parse_delimited(&mut self) -> Vec<OMathElement> {
        let begin = self.read_delimiter();
        let body = self.parse_sequence(None, true);
        // Expect `\right<delim>`.
        let end = if self.looks_at_command("right") {
            self.bump(); // '\'
            self.read_command_name(); // "right"
            self.read_delimiter()
        } else {
            String::new()
        };
        vec![OMathElement::Delimited { begin, end, body }]
    }

    /// Read the single delimiter token after `\left` / `\right` (e.g. `(`, `]`,
    /// `\{`, or `.` meaning "no delimiter").
    fn read_delimiter(&mut self) -> String {
        self.skip_spaces();
        match self.peek() {
            Some('\\') => {
                self.bump();
                // `\{` / `\}` / `\langle` etc.
                if let Some(c) = self.peek() {
                    if !c.is_alphabetic() {
                        self.bump();
                        return control_symbol(c).unwrap_or("").to_string();
                    }
                }
                let name = self.read_command_name();
                command_text(&name).unwrap_or("").to_string()
            }
            Some('.') => {
                self.bump();
                String::new() // `.` = empty delimiter
            }
            Some(c) => {
                self.bump();
                c.to_string()
            }
            None => String::new(),
        }
    }

    /// An n-ary operator (`\sum`, `\int`, …): consume `_`/`^` limits then a
    /// single scripted body atom (so `\sum a_i` subscripts the `a`, not the sum).
    fn parse_nary(&mut self, operator: String) -> Vec<OMathElement> {
        let mut sub = Vec::new();
        let mut sup = Vec::new();
        loop {
            self.skip_spaces();
            match self.peek() {
                Some('_') if sub.is_empty() => {
                    self.bump();
                    sub = self.parse_script_arg();
                }
                Some('^') if sup.is_empty() => {
                    self.bump();
                    sup = self.parse_script_arg();
                }
                _ => break,
            }
        }
        self.skip_spaces();
        let body = self.parse_scripted_atom();
        vec![OMathElement::Nary {
            operator,
            sub,
            sup,
            body,
        }]
    }

    /// Parse a required `{...}` argument (treating a bare next token as the
    /// argument if no brace is present, matching TeX's single-token rule).
    fn parse_group_arg(&mut self) -> Vec<OMathElement> {
        self.skip_spaces();
        if self.peek() == Some('{') {
            self.bump();
            let inner = self.parse_sequence(Some('}'), false);
            self.eat('}');
            inner
        } else {
            self.parse_atom()
        }
    }

    fn read_command_name(&mut self) -> String {
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphabetic() {
                name.push(c);
                self.bump();
            } else {
                break;
            }
        }
        name
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t') | Some('\n')) {
            self.bump();
        }
    }

    /// True if the input at the cursor is `\<word>` (without consuming).
    fn looks_at_command(&self, word: &str) -> bool {
        if self.peek() != Some('\\') {
            return false;
        }
        let mut i = self.pos + 1;
        for wc in word.chars() {
            if self.chars.get(i).copied() != Some(wc) {
                return false;
            }
            i += 1;
        }
        // Ensure the command name ends here (next char is not a letter).
        !self.chars.get(i).copied().is_some_and(|c| c.is_alphabetic())
    }
}

/// Coalesce adjacent `Run` nodes into one, so `x+y` is a single run rather than
/// three. Recurses into child slots.
fn merge_runs(elements: Vec<OMathElement>) -> Vec<OMathElement> {
    let mut out: Vec<OMathElement> = Vec::with_capacity(elements.len());
    for el in elements {
        let el = recurse_merge(el);
        if let (Some(OMathElement::Run(prev)), OMathElement::Run(cur)) = (out.last_mut(), &el) {
            prev.push_str(cur);
        } else {
            out.push(el);
        }
    }
    out
}

fn recurse_merge(el: OMathElement) -> OMathElement {
    match el {
        OMathElement::Run(_) => el,
        OMathElement::SuperScript { base, sup } => OMathElement::SuperScript {
            base: merge_runs(base),
            sup: merge_runs(sup),
        },
        OMathElement::SubScript { base, sub } => OMathElement::SubScript {
            base: merge_runs(base),
            sub: merge_runs(sub),
        },
        OMathElement::SubSuperScript { base, sub, sup } => OMathElement::SubSuperScript {
            base: merge_runs(base),
            sub: merge_runs(sub),
            sup: merge_runs(sup),
        },
        OMathElement::Fraction {
            numerator,
            denominator,
        } => OMathElement::Fraction {
            numerator: merge_runs(numerator),
            denominator: merge_runs(denominator),
        },
        OMathElement::Radical { degree, radicand } => OMathElement::Radical {
            degree: degree.map(merge_runs),
            radicand: merge_runs(radicand),
        },
        OMathElement::Nary {
            operator,
            sub,
            sup,
            body,
        } => OMathElement::Nary {
            operator,
            sub: merge_runs(sub),
            sup: merge_runs(sup),
            body: merge_runs(body),
        },
        OMathElement::Delimited { begin, end, body } => OMathElement::Delimited {
            begin,
            end,
            body: merge_runs(body),
        },
        OMathElement::Function { name, body } => OMathElement::Function {
            name: merge_runs(name),
            body: merge_runs(body),
        },
    }
}

/// Map an n-ary command to its operator glyph.
fn nary_operator(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "sum" => "\u{2211}",     // ∑
        "prod" => "\u{220F}",    // ∏
        "coprod" => "\u{2210}",  // ∐
        "int" => "\u{222B}",     // ∫
        "iint" => "\u{222C}",    // ∬
        "iiint" => "\u{222D}",   // ∭
        "oint" => "\u{222E}",    // ∮
        "bigcup" => "\u{22C3}",  // ⋃
        "bigcap" => "\u{22C2}",  // ⋂
        "bigoplus" => "\u{2A01}", // ⨁
        "bigotimes" => "\u{2A02}", // ⨂
        _ => return None,
    })
}

/// Map a control symbol (backslash + non-letter) to its text. Returns `Some("")`
/// for spacing commands that produce no glyph.
fn control_symbol(c: char) -> Option<&'static str> {
    Some(match c {
        '{' => "{",
        '}' => "}",
        '%' => "%",
        '&' => "&",
        '$' => "$",
        '#' => "#",
        '_' => "_",
        ' ' => "\u{00A0}", // explicit space
        ',' => "\u{2009}", // thin space
        ';' => "\u{2005}",
        ':' => "\u{2005}",
        '!' => "",         // negative thin space → drop
        '\\' => "\n",      // `\\` line break (rare in inline math)
        _ => return None,
    })
}

/// Map a command name to its rendered text: Greek letters, operators, relations,
/// arrows, function names, etc. Returns `Some("")` to drop a no-op command.
fn command_text(name: &str) -> Option<&'static str> {
    Some(match name {
        // Lowercase Greek
        "alpha" => "\u{03B1}",
        "beta" => "\u{03B2}",
        "gamma" => "\u{03B3}",
        "delta" => "\u{03B4}",
        "epsilon" => "\u{03B5}",
        "varepsilon" => "\u{03B5}",
        "zeta" => "\u{03B6}",
        "eta" => "\u{03B7}",
        "theta" => "\u{03B8}",
        "vartheta" => "\u{03D1}",
        "iota" => "\u{03B9}",
        "kappa" => "\u{03BA}",
        "lambda" => "\u{03BB}",
        "mu" => "\u{03BC}",
        "nu" => "\u{03BD}",
        "xi" => "\u{03BE}",
        "omicron" => "\u{03BF}",
        "pi" => "\u{03C0}",
        "varpi" => "\u{03D6}",
        "rho" => "\u{03C1}",
        "varrho" => "\u{03F1}",
        "sigma" => "\u{03C3}",
        "varsigma" => "\u{03C2}",
        "tau" => "\u{03C4}",
        "upsilon" => "\u{03C5}",
        "phi" => "\u{03C6}",
        "varphi" => "\u{03D5}",
        "chi" => "\u{03C7}",
        "psi" => "\u{03C8}",
        "omega" => "\u{03C9}",
        // Uppercase Greek
        "Gamma" => "\u{0393}",
        "Delta" => "\u{0394}",
        "Theta" => "\u{0398}",
        "Lambda" => "\u{039B}",
        "Xi" => "\u{039E}",
        "Pi" => "\u{03A0}",
        "Sigma" => "\u{03A3}",
        "Upsilon" => "\u{03A5}",
        "Phi" => "\u{03A6}",
        "Psi" => "\u{03A8}",
        "Omega" => "\u{03A9}",
        // Binary operators
        "times" => "\u{00D7}",
        "div" => "\u{00F7}",
        "pm" => "\u{00B1}",
        "mp" => "\u{2213}",
        "cdot" => "\u{22C5}",
        "ast" => "\u{2217}",
        "star" => "\u{22C6}",
        "circ" => "\u{2218}",
        "bullet" => "\u{2219}",
        "oplus" => "\u{2295}",
        "ominus" => "\u{2296}",
        "otimes" => "\u{2297}",
        "oslash" => "\u{2298}",
        "odot" => "\u{2299}",
        "cup" => "\u{222A}",
        "cap" => "\u{2229}",
        "setminus" => "\u{2216}",
        "wedge" => "\u{2227}",
        "land" => "\u{2227}",
        "vee" => "\u{2228}",
        "lor" => "\u{2228}",
        // Relations
        "leq" => "\u{2264}",
        "le" => "\u{2264}",
        "geq" => "\u{2265}",
        "ge" => "\u{2265}",
        "neq" => "\u{2260}",
        "ne" => "\u{2260}",
        "equiv" => "\u{2261}",
        "approx" => "\u{2248}",
        "cong" => "\u{2245}",
        "simeq" => "\u{2243}",
        "sim" => "\u{223C}",
        "propto" => "\u{221D}",
        "ll" => "\u{226A}",
        "gg" => "\u{226B}",
        "subset" => "\u{2282}",
        "supset" => "\u{2283}",
        "subseteq" => "\u{2286}",
        "supseteq" => "\u{2287}",
        "in" => "\u{2208}",
        "notin" => "\u{2209}",
        "ni" => "\u{220B}",
        "perp" => "\u{27C2}",
        "parallel" => "\u{2225}",
        "mid" => "\u{2223}",
        // Arrows
        "rightarrow" => "\u{2192}",
        "to" => "\u{2192}",
        "leftarrow" => "\u{2190}",
        "gets" => "\u{2190}",
        "leftrightarrow" => "\u{2194}",
        "Rightarrow" => "\u{21D2}",
        "implies" => "\u{21D2}",
        "Leftarrow" => "\u{21D0}",
        "Leftrightarrow" => "\u{21D4}",
        "iff" => "\u{21D4}",
        "mapsto" => "\u{21A6}",
        "uparrow" => "\u{2191}",
        "downarrow" => "\u{2193}",
        // Misc symbols
        "infty" => "\u{221E}",
        "partial" => "\u{2202}",
        "nabla" => "\u{2207}",
        "forall" => "\u{2200}",
        "exists" => "\u{2203}",
        "nexists" => "\u{2204}",
        "emptyset" => "\u{2205}",
        "varnothing" => "\u{2205}",
        "angle" => "\u{2220}",
        "triangle" => "\u{25B3}",
        "hbar" => "\u{210F}",
        "ell" => "\u{2113}",
        "Re" => "\u{211C}",
        "Im" => "\u{2111}",
        "aleph" => "\u{2135}",
        "wp" => "\u{2118}",
        "neg" => "\u{00AC}",
        "lnot" => "\u{00AC}",
        "degree" => "\u{00B0}",
        "prime" => "\u{2032}",
        "dagger" => "\u{2020}",
        "ldots" => "\u{2026}",
        "dots" => "\u{2026}",
        "cdots" => "\u{22EF}",
        "vdots" => "\u{22EE}",
        "ddots" => "\u{22F1}",
        "qquad" => "\u{2003}\u{2003}",
        "quad" => "\u{2003}",
        // Function names (rendered as upright text by Word's equation engine).
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "cot" => "cot",
        "sec" => "sec",
        "csc" => "csc",
        "arcsin" => "arcsin",
        "arccos" => "arccos",
        "arctan" => "arctan",
        "sinh" => "sinh",
        "cosh" => "cosh",
        "tanh" => "tanh",
        "log" => "log",
        "ln" => "ln",
        "lg" => "lg",
        "exp" => "exp",
        "lim" => "lim",
        "limsup" => "lim sup",
        "liminf" => "lim inf",
        "max" => "max",
        "min" => "min",
        "sup" => "sup",
        "inf" => "inf",
        "arg" => "arg",
        "deg" => "deg",
        "det" => "det",
        "dim" => "dim",
        "gcd" => "gcd",
        "ker" => "ker",
        "mod" => "mod",
        // Accent/format commands we can't render structurally: pass the argument
        // through by dropping the command (the following group renders normally).
        "mathrm" | "mathbf" | "mathit" | "mathsf" | "mathcal" | "mathbb" | "boldsymbol"
        | "text" | "operatorname" | "displaystyle" | "limits" | "nolimits" | "left." => "",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the equation and return its document.xml fragment for assertions.
    fn xml(src: &str, display: bool) -> String {
        use docx_rs::BuildXML;
        let math = latex_to_omath(src, display);
        String::from_utf8(math.build()).unwrap()
    }

    #[test]
    fn plain_text_is_one_run() {
        let s = xml("a+b=c", false);
        assert!(s.contains("<m:oMath>"));
        assert!(s.contains("a+b=c"), "adjacent chars merged into one run");
    }

    #[test]
    fn superscript() {
        let s = xml("x^2", false);
        assert!(s.contains("<m:sSup>"));
        assert!(s.contains("<m:sup>"));
    }

    #[test]
    fn subscript_and_superscript() {
        let s = xml("x_i^2", false);
        assert!(s.contains("<m:sSubSup>"));
    }

    #[test]
    fn braced_superscript_groups() {
        let s = xml("e^{i\\pi}", false);
        assert!(s.contains("<m:sSup>"));
        assert!(s.contains("\u{03C0}"), "pi glyph present");
    }

    #[test]
    fn fraction() {
        let s = xml("\\frac{a}{b}", false);
        assert!(s.contains("<m:f>"));
        assert!(s.contains("<m:num>"));
        assert!(s.contains("<m:den>"));
    }

    #[test]
    fn sqrt_and_nth_root() {
        let plain = xml("\\sqrt{2}", false);
        assert!(plain.contains("<m:rad>"));
        assert!(plain.contains("m:val=\"1\""), "sqrt hides degree");

        let nth = xml("\\sqrt[3]{x}", false);
        assert!(nth.contains("<m:deg>"));
    }

    #[test]
    fn sum_with_limits() {
        let s = xml("\\sum_{i=1}^{n} i", false);
        assert!(s.contains("<m:nary>"));
        assert!(s.contains("\u{2211}"), "sum glyph");
        assert!(s.contains("<m:sub>"));
        assert!(s.contains("<m:sup>"));
    }

    #[test]
    fn delimiters() {
        let s = xml("\\left( x \\right)", false);
        assert!(s.contains("<m:d>"));
        assert!(s.contains("m:begChr"));
    }

    #[test]
    fn unknown_command_is_not_lost() {
        let s = xml("\\foobar", false);
        assert!(s.contains("foobar"), "unknown command kept as text");
    }

    #[test]
    fn display_wraps_in_para() {
        let s = xml("x^2", true);
        assert!(s.contains("<m:oMathPara>"));
    }

    #[test]
    fn empty_input_does_not_panic() {
        let s = xml("", false);
        assert!(s.contains("<m:oMath>"));
    }

    #[test]
    fn greek_and_relations() {
        let s = xml("\\alpha \\leq \\beta", false);
        assert!(s.contains("\u{03B1}"));
        assert!(s.contains("\u{2264}"));
        assert!(s.contains("\u{03B2}"));
    }

    #[test]
    fn deep_nesting_does_not_overflow_the_stack() {
        // Adversarial input: tens of thousands of nested braces must not blow the
        // stack. The depth guard turns the overflow into bounded literal text.
        let src = format!("{}x{}", "{".repeat(50_000), "}".repeat(50_000));
        let s = xml(&src, false);
        assert!(s.contains("<m:oMath>"), "still produces a (bounded) equation");
    }

    #[test]
    fn nary_body_keeps_its_own_scripts() {
        // `\sum a_i` must subscript the `a` (inside the n-ary body), not the sum.
        let s = xml("\\sum_{i=1}^{n} a_i^2", false);
        let nary = s.find("<m:nary>").expect("nary present");
        assert!(
            s[nary..].contains("<m:sSubSup>"),
            "the body a_i^2 builds a sub/superscript inside the n-ary"
        );
    }

    #[test]
    fn stray_right_does_not_corrupt_a_group() {
        // A `\right` with no enclosing `\left` is literal text, and the `{...}`
        // group structure (and its content) must survive intact.
        let s = xml("{a \\right b}", false);
        assert!(s.contains("<m:oMath>"));
        assert!(s.contains('a') && s.contains('b'), "group content kept: {s}");
        assert!(!s.contains("}"), "the literal close brace must not leak");
    }

    #[test]
    fn malformed_input_never_panics() {
        // None of these may panic, overflow, or hang; each yields some oMath.
        for c in [
            "\\", "{", "}", "{{{", "}}}", "^", "_", "x^", "x_", "\\frac", "\\frac{a}", "\\sqrt",
            "\\sqrt[", "\\sqrt[3]", "\\left(", "\\right)", "\\left( x", "\\sum", "\\sum_",
            "\\sum_{", "a^^b", "a~~b", "\\\\", "{}", "_^_^", "\\frac{}{}", "\\left.\\right.",
        ] {
            assert!(xml(c, false).contains("<m:oMath>"), "no equation for {c:?}");
        }
    }

    #[test]
    fn multibyte_content_is_handled() {
        let s = xml("\u{00E9}^{\u{00E9}}", false);
        assert!(s.contains("<m:sSup>"));
        assert!(s.contains('\u{00E9}'));
    }
}
