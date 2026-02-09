use std::thread;
use std::time::Duration;

#[cfg(windows)]
use uiautomation::patterns::UIInvokePattern;
#[cfg(windows)]
use uiautomation::types::{TreeScope, UIProperty};
#[cfg(windows)]
use uiautomation::variants::Variant;
#[cfg(windows)]
use uiautomation::{UIElement, core::UIAutomation};

#[cfg(windows)]
pub struct UIAutomationControl {
    automation: UIAutomation,
    window: UIElement,
}

#[cfg(windows)]
unsafe impl Send for UIAutomationControl {}
#[cfg(windows)]
unsafe impl Sync for UIAutomationControl {}

#[cfg(windows)]
#[allow(dead_code)]
impl UIAutomationControl {
    #[allow(dead_code)]
    pub fn new(window_name_prefix: &str) -> uiautomation::Result<Self> {
        Self::new_with_pid(window_name_prefix, None)
    }

    /// Get a reference to the UIAutomation instance
    pub fn automation(&self) -> &UIAutomation {
        &self.automation
    }

    /// Get a reference to the window element
    pub fn window(&self) -> &UIElement {
        &self.window
    }

    /// Focus the window
    pub fn focus_window(&self) -> uiautomation::Result<()> {
        self.window.set_focus()
    }

    /// Check if an element with the given class name prefix exists
    pub fn has_element_with_class_prefix(&self, class_name_prefix: &str) -> bool {
        let true_condition = match self.automation.create_true_condition() {
            Ok(c) => c,
            Err(_) => return false,
        };
        let all_elements = match self.window.find_all(TreeScope::Descendants, &true_condition) {
            Ok(e) => e,
            Err(_) => return false,
        };
        all_elements.iter().any(|el| {
            el.get_classname()
                .map(|c| c.starts_with(class_name_prefix))
                .unwrap_or(false)
        })
    }

    pub fn new_with_pid(window_name_prefix: &str, pid: Option<u32>) -> uiautomation::Result<Self> {
        let automation = UIAutomation::new()?;

        let root = automation.get_root_element()?;
        let window_condition = automation.create_property_condition(
            UIProperty::ControlType,
            Variant::from(uiautomation::controls::ControlType::Window as i32),
            None,
        )?;

        let windows = root.find_all(TreeScope::Children, &window_condition)?;

        let filtered_windows: Vec<_> = windows
            .into_iter()
            .filter(|w| {
                w.get_name()
                    .map(|n| n.starts_with(window_name_prefix))
                    .unwrap_or(false)
            })
            .collect();

        //
        // print details from all filtered windows for debugging
        // for w in &filtered_windows {
        //    let name = w.get_name().unwrap_or_default();
        //    let class = w.get_classname().unwrap_or_default();
        //    let process_id = w.get_process_id().unwrap_or_default();
        //    println!("Window - Name: '{}', Class: '{}', PID: {}", name, class,
        // process_id);
        // }.
        //

        let window = if let Some(pid) = pid {
            filtered_windows
                .into_iter()
                .find(|w| w.get_process_id().map(|p| p as u32 == pid).unwrap_or(false))
                .ok_or_else(|| {
                    common::log_warn!("Window not found for PID {} with prefix '{}'", pid, window_name_prefix);
                    uiautomation::Error::new(
                        uiautomation::errors::ERR_NOTFOUND,
                        "Window not found for PID",
                    )
                })?
        } else {
            filtered_windows
                .into_iter()
                .next()
                .ok_or_else(|| {
                    common::log_error!("Window not found with prefix '{}'", window_name_prefix);
                    uiautomation::Error::new(
                        uiautomation::errors::ERR_NOTFOUND,
                        "Window not found",
                    )
                })?
        };

        Ok(Self { automation, window })
    }

    #[allow(dead_code)]
    pub fn invoke_element(
        &self,
        class_name_prefix: &str,
        name_substring: &str,
    ) -> uiautomation::Result<()> {
        self.invoke_element_with_wait(class_name_prefix, name_substring, 500)
    }

    #[allow(dead_code)]
    pub fn find_element(
        &self,
        class_name_prefix: &str,
        name_substring: &str,
    ) -> uiautomation::Result<UIElement> {
        let true_condition = self.automation.create_true_condition()?;
        let all_elements = self
            .window
            .find_all(TreeScope::Descendants, &true_condition)?;

        let target_element = all_elements
            .into_iter()
            .find(|el| {
                let class_match = el
                    .get_classname()
                    .map(|c| c.starts_with(class_name_prefix))
                    .unwrap_or(false);
                let name_match = el
                    .get_name()
                    .map(|n| n.to_lowercase().contains(name_substring))
                    .unwrap_or(false);
                class_match && name_match
            })
            .ok_or_else(|| {
                common::log_error!("Element not found with class prefix '{}' and name substring '{}'", class_name_prefix, name_substring);
                uiautomation::Error::new(
                    uiautomation::errors::ERR_NOTFOUND,
                    "Element not found",
                )
            })?;

        Ok(target_element)
    }

    #[allow(dead_code)]
    pub fn invoke_element_with_wait(
        &self,
        class_name_prefix: &str,
        name_substring: &str,
        wait: u64,
    ) -> uiautomation::Result<()> {
        let element = self.find_element(class_name_prefix, name_substring)?;

        let invoke_pattern: UIInvokePattern = element.get_pattern()?;
        invoke_pattern.invoke()?;

        thread::sleep(Duration::from_millis(wait));

        Ok(())
    }

    pub fn send_keys(&self, class_name_prefix: &str, keys: &str) -> uiautomation::Result<()> {
        let true_condition = self.automation.create_true_condition()?;
        let all_elements = self
            .window
            .find_all(TreeScope::Descendants, &true_condition)?;

        let target_element = all_elements
            .into_iter()
            .find(|el| {
                el.get_classname()
                    .map(|c| c.starts_with(class_name_prefix))
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                common::log_error!("Element not found for send_keys with class prefix '{}'", class_name_prefix);
                uiautomation::Error::new(
                    uiautomation::errors::ERR_NOTFOUND,
                    "Element not found",
                )
            })?;

        target_element.set_focus()?;
        target_element.send_keys(keys, 10)?;

        Ok(())
    }

    pub fn send_text(&self, class_name_prefix: &str, keys: &str) -> uiautomation::Result<()> {
        let true_condition = self.automation.create_true_condition()?;
        let all_elements = self
            .window
            .find_all(TreeScope::Descendants, &true_condition)?;

        let target_element = all_elements
            .iter()
            .find(|el| {
                el.get_classname()
                    .map(|c| c.starts_with(class_name_prefix))
                    .unwrap_or(false)
            })
            .cloned()
            .ok_or_else(|| {
                common::log_error!("Element not found for send_text with class prefix '{}'", class_name_prefix);
                //
                // Log all available elements for debugging.
                //
                for el in &all_elements {
                    let class = el.get_classname().unwrap_or_default();
                    let name = el.get_name().unwrap_or_default();
                    let ctrl_type = el.get_control_type().map(|t| format!("{:?}", t)).unwrap_or_default();
                    if !class.is_empty() || !name.is_empty() {
                        common::log_warn!("  Available element - class: '{}', name: '{}', type: {}", class, name, ctrl_type);
                    }
                }
                uiautomation::Error::new(
                    uiautomation::errors::ERR_NOTFOUND,
                    "Element not found",
                )
            })?;

        target_element.set_focus()?;
        target_element.send_text(keys, 10)?;

        Ok(())
    }
}
