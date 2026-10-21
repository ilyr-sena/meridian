//! Load and use fonts.
use std::hash::Hash;

/// A font.
///
/// `PartialEq`/`Eq`/`Hash` are implemented manually (not derived) so the
/// `letter_spacing` float can live in the struct: the spacing is compared
/// and hashed through its bit pattern, which keeps the derived semantics
/// while satisfying `Eq`/`Hash` for a field of type `f32`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Font {
    /// The [`Family`] of the [`Font`].
    pub family: Family,
    /// The [`Weight`] of the [`Font`].
    pub weight: Weight,
    /// The [`Stretch`] of the [`Font`].
    pub stretch: Stretch,
    /// The [`Style`] of the [`Font`].
    pub style: Style,
    /// Extra spacing between characters, in EM — a fraction of the font
    /// size (matching cosmic-text's `Attrs::letter_spacing`).
    ///
    /// Negative values tighten the tracking; `0.0` keeps the font's natural
    /// spacing. Example: `-0.025` removes ~0.4px per glyph at a 16px size.
    pub letter_spacing: f32,
}

impl Font {
    /// A non-monospaced sans-serif font with normal [`Weight`].
    pub const DEFAULT: Font = Font {
        family: Family::SansSerif,
        weight: Weight::Normal,
        stretch: Stretch::Normal,
        style: Style::Normal,
        letter_spacing: 0.0,
    };

    /// A monospaced font with normal [`Weight`].
    pub const MONOSPACE: Font = Font {
        family: Family::Monospace,
        ..Self::DEFAULT
    };

    /// Creates a non-monospaced [`Font`] with the given [`Family::Name`] and
    /// normal [`Weight`].
    pub const fn with_name(name: &'static str) -> Self {
        Font {
            family: Family::Name(name),
            ..Self::DEFAULT
        }
    }

    /// Sets the [`Font::letter_spacing`] of the [`Font`].
    ///
    /// Spacing is expressed in EM (a fraction of the font size) and is applied
    /// on top of the font's natural advances (negative values tighten the
    /// tracking) — same units as cosmic-text's `Attrs::letter_spacing`.
    pub const fn letter_spacing(mut self, letter_spacing: f32) -> Self {
        self.letter_spacing = letter_spacing;
        self
    }
}

impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.family == other.family
            && self.weight == other.weight
            && self.stretch == other.stretch
            && self.style == other.style
            && self.letter_spacing.to_bits() == other.letter_spacing.to_bits()
    }
}

impl Eq for Font {}

impl Hash for Font {
    fn hash<__H: std::hash::Hasher>(&self, state: &mut __H) {
        self.family.hash(state);
        self.weight.hash(state);
        self.stretch.hash(state);
        self.style.hash(state);
        self.letter_spacing.to_bits().hash(state);
    }
}

/// A font family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Family {
    /// The name of a font family of choice.
    Name(&'static str),

    /// Serif fonts represent the formal text style for a script.
    Serif,

    /// Glyphs in sans-serif fonts, as the term is used in CSS, are generally low
    /// contrast and have stroke endings that are plain — without any flaring,
    /// cross stroke, or other ornamentation.
    #[default]
    SansSerif,

    /// Glyphs in cursive fonts generally use a more informal script style, and
    /// the result looks more like handwritten pen or brush writing than printed
    /// letterwork.
    Cursive,

    /// Fantasy fonts are primarily decorative or expressive fonts that contain
    /// decorative or expressive representations of characters.
    Fantasy,

    /// The sole criterion of a monospace font is that all glyphs have the same
    /// fixed width.
    Monospace,
}

/// The weight of some text.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Weight {
    Thin,
    ExtraLight,
    Light,
    #[default]
    Normal,
    Medium,
    Semibold,
    Bold,
    ExtraBold,
    Black,
}

/// The width of some text.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Stretch {
    UltraCondensed,
    ExtraCondensed,
    Condensed,
    SemiCondensed,
    #[default]
    Normal,
    SemiExpanded,
    Expanded,
    ExtraExpanded,
    UltraExpanded,
}

/// The style of some text.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Style {
    #[default]
    Normal,
    Italic,
    Oblique,
}
