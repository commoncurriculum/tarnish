//! Inline styles through tarnish-css, kept by their text: a document's styles repeat, as a color
//! set on many headings does, and the CSS engine takes microseconds to parse one.

use std::cell::{OnceCell, RefCell};
use std::sync::Arc;

use rustc_hash::FxHashMap;
use tarnish_css::Declarations;

/// The most styles a thread keeps before it starts over.
const KEPT: usize = 512;

/// The longest style kept. The styles that repeat are short, and a long one, such as a pasted
/// `background-image` with a data URL, would stay on the thread until the cache starts over.
const LONGEST: usize = 256;

struct Style {
    declarations: Arc<Declarations>,
    css_text: OnceCell<Arc<str>>,
}

impl Style {
    fn parse(css: &str) -> Style {
        Style {
            declarations: Arc::new(Declarations::parse(css)),
            css_text: OnceCell::new(),
        }
    }
}

thread_local! {
    static STYLES: RefCell<FxHashMap<Box<str>, Style>> = RefCell::default();
}

fn with_style<R>(css: &str, read: impl FnOnce(&Style) -> R) -> R {
    if css.len() > LONGEST {
        return read(&Style::parse(css));
    }
    STYLES.with_borrow_mut(|styles| {
        if let Some(style) = styles.get(css) {
            return read(style);
        }
        if styles.len() >= KEPT {
            styles.clear();
        }
        read(styles.entry(css.into()).or_insert(Style::parse(css)))
    })
}

/// The declarations of a `style` attribute, as `element.style` holds them.
pub(crate) fn declarations(css: &str) -> Arc<Declarations> {
    with_style(css, |style| Arc::clone(&style.declarations))
}

/// What `style.cssText = css` sets the `style` attribute to.
pub(crate) fn css_text(css: &str) -> Arc<str> {
    with_style(css, |style| {
        Arc::clone(
            style
                .css_text
                .get_or_init(|| style.declarations.css_text().into()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_short_styles() {
        let long = format!("background-image: url(data:,{})", "a".repeat(LONGEST));
        for css in [long.as_str(), "color: red"] {
            declarations(css);
            css_text(css);
        }
        STYLES.with_borrow(|styles| {
            assert!(!styles.contains_key(long.as_str()));
            assert!(styles["color: red"].css_text.get().is_some());
        });
    }
}
