/// # Custom Markdown Filter
///
/// askama has a Markdown filter, but that relies on comrak.
/// While comrak is generally great for parsing and rendering
/// Markdown, it lacks some configuration options tome needs (specifically,
/// rewriting broken links). This means we use a custom filter to
/// render Markdown using the pulldown_cmark crate.
use askama::MarkupDisplay;
use pulldown_cmark::{html, BrokenLink, CowStr, Event, LinkType, Options, Tag};

fn handle_broken_link(broken_link: BrokenLink<'_>) -> Option<(CowStr<'_>, CowStr<'_>)> {
    Some((broken_link.reference.clone(), broken_link.reference))
}

pub fn custom_md<S>(s: S) -> askama::Result<MarkupDisplay<askama_escape::Html, String>>
where
    S: AsRef<str>,
{
    let mut binding = handle_broken_link;
    let parser = pulldown_cmark::Parser::new_with_broken_link_callback(
        s.as_ref(),
        Options::all(),
        Some(&mut binding),
    )
    .map(|event| match event {
        Event::Start(tag) => {
            let tag = match tag {
                Tag::Link(link_type, dest, title) => {
                    let dest = if link_type == LinkType::ShortcutUnknown {
                        format!("/article/{dest}")
                    } else {
                        dest.to_string()
                    };
                    dbg!(&link_type);
                    dbg!(&dest);
                    Tag::Link(link_type, dest.into(), title)
                }
                _ => tag,
            };
            Event::Start(tag)
        }
        _ => event,
    });
    let mut html_out = String::new();
    html::push_html(&mut html_out, parser);
    Ok(MarkupDisplay::new_safe(html_out, askama_escape::Html))
}
