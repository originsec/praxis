//
// M365 Copilot-specific message handling functions
// These work with the UIAutomationControl to extract chat messages from the
// M365 Copilot UI.
//

use crate::utils::UIAutomationControl;
use uiautomation::UIElement;
use uiautomation::patterns::UITextPattern;
use uiautomation::types::{TreeScope, UIProperty};
use uiautomation::variants::Variant;

/// Find all message elements (Group elements with class name starting with prefix)
pub fn find_messages_by_class_prefix(
    ctrl: &UIAutomationControl,
    class_prefix: &str,
) -> uiautomation::Result<Vec<UIElement>> {
    let automation = ctrl.automation();
    let window = ctrl.window();

    let group_condition = automation.create_property_condition(
        UIProperty::ControlType,
        Variant::from(uiautomation::controls::ControlType::Group as i32),
        None,
    )?;

    let all_groups = window.find_all(TreeScope::Descendants, &group_condition)?;

    let filtered: Vec<_> = all_groups
        .into_iter()
        .filter(|el| {
            el.get_classname()
                .map(|cn| cn.starts_with(class_prefix))
                .unwrap_or(false)
        })
        .collect();

    Ok(filtered)
}

/// Count ToolBar elements with class name starting with prefix (indicates message is complete)
pub fn count_toolbars_by_class_prefix(
    ctrl: &UIAutomationControl,
    class_prefix: &str,
) -> uiautomation::Result<usize> {
    let automation = ctrl.automation();
    let window = ctrl.window();

    let toolbar_condition = automation.create_property_condition(
        UIProperty::ControlType,
        Variant::from(uiautomation::controls::ControlType::ToolBar as i32),
        None,
    )?;

    let all_toolbars = window.find_all(TreeScope::Descendants, &toolbar_condition)?;

    let count = all_toolbars
        .into_iter()
        .filter(|el| {
            el.get_classname()
                .map(|cn| cn.starts_with(class_prefix))
                .unwrap_or(false)
        })
        .count();

    Ok(count)
}

/// Get the content element from a message element
/// Navigate: message -> first Group child -> first Group child (content container)
pub fn get_message_content_element(
    ctrl: &UIAutomationControl,
    message_element: &UIElement,
) -> uiautomation::Result<Option<UIElement>> {
    let automation = ctrl.automation();

    let group_condition = automation.create_property_condition(
        UIProperty::ControlType,
        Variant::from(uiautomation::controls::ControlType::Group as i32),
        None,
    )?;

    //
    // First level: direct Group children of message.
    //
    let first_level = message_element.find_all(TreeScope::Children, &group_condition)?;
    let first_group = match first_level.into_iter().next() {
        Some(g) => g,
        None => return Ok(None),
    };

    //
    // Second level: Group children of first group (this contains the actual
    // content).
    //
    let second_level = first_group.find_all(TreeScope::Children, &group_condition)?;
    Ok(second_level.into_iter().next())
}

/// Extract text from a message content element
pub fn extract_message_text(
    ctrl: &UIAutomationControl,
    content_element: &UIElement,
) -> uiautomation::Result<Option<String>> {
    let automation = ctrl.automation();
    let true_condition = automation.create_true_condition()?;
    let descendants = content_element.find_all(TreeScope::Descendants, &true_condition)?;

    if descendants.is_empty() {
        return Ok(None);
    }

    //
    // Check if still generating.
    //
    for elem in &descendants {
        if let Ok(name) = elem.get_name() {
            if name.contains("Generating response") || name.contains("search for") {
                //
                // Still generating.
                //
                return Ok(None);
            }
        }
    }

    let mut text = String::new();

    for elem in &descendants {
        let control_type = elem.get_control_type()?;

        match control_type {
            uiautomation::controls::ControlType::Separator => {
                text.push_str("\n\n");
            }
            uiautomation::controls::ControlType::List => {
                text.push('\n');
            }
            uiautomation::controls::ControlType::ListItem => {
                text.push_str("\n• ");
            }
            uiautomation::controls::ControlType::Text => {
                //
                // Try Text pattern first (get_document_range -> get_text), fall
                // back to Name.
                //
                let elem_text = elem
                    .get_pattern::<UITextPattern>()
                    .and_then(|tp| tp.get_document_range())
                    .and_then(|range| range.get_text(-1))
                    .or_else(|_| elem.get_name())
                    .unwrap_or_default();
                if !elem_text.is_empty() {
                    text.push_str(&elem_text);
                }
            }
            uiautomation::controls::ControlType::Hyperlink => {
                if let Ok(name) = elem.get_name() {
                    text.push_str(&name);
                }
            }
            uiautomation::controls::ControlType::Button => {
                text.push('\n');
            }
            _ => {}
        }
    }

    Ok(Some(text.trim().to_string()))
}
