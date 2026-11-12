//! `SafeHtml`: the single place where external HTML (patch notes) is inserted. The HTML is parsed into an inert
//! `<template>` (no script runs, no image loads), walked against the allowlists of `core::sanitize`, and the
//! cleaned fragment replaces the children of the element; no HTML string is ever injected as it is.
//! Replaces `components/SafeHtml.tsx` (DOMPurify).

use leptos::{prelude::*, wasm_bindgen::JsCast, web_sys};
use storm_web_core::sanitize::{attr_allowed, style_safe, tag_allowed, tag_dropped, url_safe};

/// Removes from `parent` everything the allowlists refuse: comments, active content with its content, unknown tags
/// (unwrapped, their text stays), forbidden attributes and unsafe links or styles.
fn clean(parent: &web_sys::Node, keep_ids: bool) {
    let mut child = parent.first_child();
    while let Some(node) = child {
        let next = node.next_sibling();
        match node.node_type() {
            web_sys::Node::TEXT_NODE => {}
            web_sys::Node::ELEMENT_NODE => {
                if let Some(el) = node.dyn_ref::<web_sys::Element>() {
                    let tag = el.tag_name();
                    if tag_dropped(&tag) {
                        let _ = parent.remove_child(&node);
                    } else if tag_allowed(&tag) {
                        clean_attributes(el, &tag, keep_ids);
                        clean(&node, keep_ids);
                    } else {
                        clean(&node, keep_ids);
                        while let Some(inner) = node.first_child() {
                            let _ = parent.insert_before(&inner, Some(&node));
                        }
                        let _ = parent.remove_child(&node);
                    }
                }
            }
            _ => {
                let _ = parent.remove_child(&node);
            }
        }
        child = next;
    }
}

fn clean_attributes(el: &web_sys::Element, tag: &str, keep_ids: bool) {
    for name in el
        .get_attribute_names()
        .iter()
        .filter_map(|n| n.as_string())
    {
        let value = el.get_attribute(&name).unwrap_or_default();
        let lower = name.to_ascii_lowercase();
        let keep = attr_allowed(tag, &name, keep_ids)
            && match lower.as_str() {
                "href" => url_safe(&value, false),
                "src" => url_safe(&value, true),
                "style" => style_safe(&value),
                _ => true,
            };
        if !keep {
            let _ = el.remove_attribute(&name);
        }
    }
}

/// Parses and sanitises `html`; `None` when the browser refuses to build the template.
fn sanitised(html: &str, keep_ids: bool) -> Option<web_sys::DocumentFragment> {
    let template = web_sys::window()?
        .document()?
        .create_element("template")
        .ok()?
        .dyn_into::<web_sys::HtmlTemplateElement>()
        .ok()?;
    template.set_inner_html(html);
    let content = template.content();
    clean(&content, keep_ids);
    Some(content)
}

/// An element whose children are the sanitised `html`. `keep_ids` keeps the `id` attributes (section anchors).
#[component]
pub fn SafeHtml(
    #[prop(into)] html: String,
    class: &'static str,
    #[prop(optional)] keep_ids: bool,
) -> impl IntoView {
    let node = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        if let Some(el) = node.get() {
            match sanitised(&html, keep_ids) {
                Some(fragment) => el.replace_children_with_node_1(&fragment),
                None => el.replace_children_with_node_0(),
            }
        }
    });
    view! { <div node_ref=node class=class></div> }
}
