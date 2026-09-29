#![forbid(unsafe_code)]

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
pub struct Declarations(tarnish_css::Declarations);

#[wasm_bindgen]
impl Declarations {
    #[wasm_bindgen(constructor)]
    pub fn new(css: &str) -> Declarations {
        Declarations(tarnish_css::Declarations::parse(css))
    }

    #[wasm_bindgen(js_name = cssText)]
    pub fn css_text(&self) -> String {
        self.0.css_text()
    }

    #[wasm_bindgen(getter)]
    pub fn length(&self) -> usize {
        self.0.len()
    }

    pub fn item(&self, index: usize) -> Option<String> {
        self.0.item(index)
    }

    pub fn names(&self) -> Vec<String> {
        self.0.names()
    }

    pub fn value(&self, name: &str) -> String {
        self.0.value(name)
    }

    pub fn priority(&self, name: &str) -> String {
        self.0.priority(name).to_owned()
    }

    pub fn set(&mut self, name: &str, value: &str, priority: &str) -> bool {
        self.0.set(name, value, priority)
    }

    pub fn remove(&mut self, name: &str) -> Option<String> {
        self.0.remove(name)
    }
}

#[wasm_bindgen(js_name = propertyNames)]
pub fn property_names() -> Vec<String> {
    tarnish_css::property_names().collect()
}

#[wasm_bindgen]
pub fn engine() -> String {
    tarnish_css::ENGINE.to_owned()
}
