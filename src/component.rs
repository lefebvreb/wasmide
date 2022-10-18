//! Reusable components for the application.
//! 
//! This module exposes the type [`Component`] which is a wrapper around an
//! HTML element. It can be used to create reusable components.

use core::any::Any;
use core::cell::UnsafeCell;
use core::mem;
use core::sync::atomic::{AtomicBool, Ordering::SeqCst};

use alloc::boxed::Box;
use alloc::rc::{Rc, Weak};
use alloc::string::ToString;
use alloc::vec::Vec;
use once_cell::unsync::Lazy;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::HtmlElement;

use crate::prelude::*;

// The internal state of a component.
struct InternalComponent {
    element: HtmlElement,
    deps: UnsafeCell<Vec<Box<dyn Any>>>,
}

impl InternalComponent {
    // Creates a new component with the given html element.
    fn new(element: HtmlElement) -> Self {
        Self {
            element,
            deps: Vec::new().into(),
        }
    }
}

// Intializes the lazy child if not done already and attaches it to the parent,
// else simply unhides the child.
fn init_and_attach<F: FnOnce() -> Component>(child: &Lazy<Component, F>, parent: &WeakComponent) {
    if let Some(child) = Lazy::get(&child) {
        child.html().set_hidden(false);
    } else {
        let child = Lazy::force(&child);
        parent.upgrade().unwrap().append_child(child.clone());
    }
}

/// Reusable component, corresponds to a single html element of the page.
/// 
/// # Examples
/// 
/// ```no_run
/// use wasmide::prelude::*;
/// 
/// let root = Component::root(Style::NONE);
/// ```
#[derive(Clone)]
pub struct Component(Rc<InternalComponent>);

impl Component {
    // Pushes a dependency to the component's storage.
    fn push_dependency<T: Any>(&self, dep: T) {
        if mem::needs_drop::<T>() {
            // SAFETY: The deps vec is never borrowed.
            unsafe { (*self.0.deps.get()).push(Box::new(dep)) };
        }
    }

    // Appends a child to the component.
    fn append_child(&self, child: Component) {
        self.html().append_child(child.html()).unwrap();
        self.push_dependency(child);
    }

    // Sets the inner html attribute of the element on store update.
    pub(crate) fn set_inner_html<S: ToString>(&self, text: impl Subscribable<S>) {
        let weak = self.downgrade();

        let unsub = text.subscribe(move |text| {
            if let Some(comp) = weak.upgrade() {
                comp.html().set_inner_html(&text.to_string());
            }
        });

        self.push_dependency(unsub.droppable());
    }

    // Converts the rust closure to a js one and sets it as the onclick property
    // of the element contained in this component.
    pub(crate) fn set_on_click(&self, on_click: impl FnMut() + 'static) {
        let on_click = Closure::wrap(Box::new(on_click) as Box<dyn FnMut()>);
        self.html().set_onclick(Some(on_click.as_ref().unchecked_ref()));
        self.push_dependency(on_click);
    }

    // Creates a new component with the given html tag_name and style.
    pub(crate) fn new(tag_name: &'static str, style: Style) -> Self {
        let element = web_sys::window().unwrap()
            .document().unwrap()
            .create_element(tag_name).unwrap()
            .dyn_into().unwrap();

        let this = Component(Rc::new(InternalComponent::new(element)));
        if style != Style::NONE {
            this.set_style(style);
        }
        this
    }

    /// Returns the root component of the application. The returned component will
    /// never get dropped. May only be called once.
    /// 
    /// # Panics
    /// 
    /// Panics if called more than once.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// use wasmide::prelude::*;
    /// 
    /// Component::root(Style::NONE)
    ///     .with(html::p(Value("Hello, world!"), Style::NONE)); 
    /// ```
    pub fn root(style: Style) -> Component {
        // For preventing data races.
        static INITIALIZED: AtomicBool = AtomicBool::new(false);
        // To prevent the body from being dropped.
        static mut ROOT: Option<Component> = None;

        INITIALIZED.compare_exchange(false, true, SeqCst, SeqCst).expect("root already initialized");
        
        let body = web_sys::window().unwrap()
            .document().unwrap()
            .body().unwrap();

        let comp = Component(Rc::new(InternalComponent::new(body)));
        comp.set_style(style);

        // SAFETY: Thread-safe thanks to the INITIALIZED atomic flag.
        unsafe { ROOT = Some(comp.clone()); }

        comp
    }

    /// Returns a reference to the html element wrapped by the component.
    /// This allows direct modification of the html but it is discouraged as
    /// it can lead to unexpected behavior. 
    /// 
    /// For example, a children of a component, appended with [`Component::with_if`]
    /// will get hidden if the condition is false. Manually making it visible will
    /// cause problems.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// use wasmide::prelude::*;
    /// 
    /// let root = Component::root(Style::NONE);
    /// let html = root.html();
    /// ```
    pub fn html(&self) -> &HtmlElement {
        &self.0.element
    }

    /// Creates a new [`WeakComponent`] reference to this component.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// use wasmide::prelude::*;
    /// 
    /// let root = Component::root(Style::NONE);
    /// let weak = root.downgrade();
    /// ```
    pub fn downgrade(&self) -> WeakComponent {
        WeakComponent(Rc::downgrade(&self.0))
    }

    /// Sets the style of the component.
    /// 
    /// This will in fact only set the class attribute of the html element
    /// to the string wrapped in the given style.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// use wasmide::prelude::*;
    /// 
    /// let root = Component::root(Style::NONE);
    /// root.set_style(Style("bg-blue-200"));
    /// ```
    pub fn set_style(&self, style: Style) {
        self.html().set_class_name(style.0);
    }

    /// Appends a child to the component. This method is meant to be chained.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// use wasmide::prelude::*;
    /// 
    /// Component::root(Style::NONE)
    ///     .with(html::p(Value("Hello, world!"), Style::NONE));
    /// ```
    pub fn with(self, component: Component) -> Self {
        self.append_child(component);
        self
    }

    /// Appends a child to the component if the condition is true.
    /// This method is meant to be chained.
    /// 
    /// When the condition becomes false, the child will be hidden, and then shown 
    /// again when it becomes true.
    /// If the consition is initially false, the child will be lazyly initialized when
    /// it becomes true.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// use wasmide::prelude::*;
    /// 
    /// let store = Store::new(100);
    /// 
    /// Component::root(Style::NONE)
    ///    .with_if(
    ///         store.compose(|n| *n > 42),
    ///         || html::p(Value("Greater than 42"), Style::NONE),
    ///   );
    /// ``` 
    pub fn with_if(
        self,
        condition: impl Subscribable<bool>,
        make_child: impl FnOnce() -> Component + 'static,
    ) -> Self {
        let div = html::div(Style::NONE);
        let weak = div.downgrade();
        self.append_child(div);

        let lazy = Lazy::new(make_child);

        let unsub = condition.subscribe(move |&condition| {
            if condition {
                init_and_attach(&lazy, &weak);
            } else {
                Lazy::get(&lazy).map(|child| child.html().set_hidden(true));
            }
        });

        self.push_dependency(unsub.droppable());
        self
    }

    /// Appends a child to the component if the condition is true, else
    /// appends another child.
    /// This method is meant to be chained.
    /// 
    /// When the condition becomes false, the first child will be hidden and the second
    /// shown. When it becomes true, the second child will be hidden and the first shown.
    /// 
    /// If the condition is initially false, the first child will be lazyly initialized when
    /// it becomes true. If it is true initially, the second child will be lazyly initialized
    /// when it becomes false.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// use wasmide::prelude::*;
    /// 
    /// let hour = Store::new(6);
    /// 
    /// Component::root(Style::NONE)
    ///    .with_if_else(
    ///         hour.compose(|h| *h > 9 && *h < 18),
    ///         || html::p(Value("Hello, world!"), Style::NONE),
    ///         || html::p(Value("Goodbye, world!"), Style::NONE),
    ///   );
    /// ``` 
    pub fn with_if_else(
        self, 
        condition: impl Subscribable<bool>, 
        make_child1: impl FnOnce() -> Component + 'static, 
        make_child2: impl FnOnce() -> Component + 'static,
    ) -> Self {
        let div = html::div(Style::NONE);
        let weak = div.downgrade();
        self.append_child(div);

        let lazy1 = Lazy::new(make_child1);
        let lazy2 = Lazy::new(make_child2);

        let unsub = condition.subscribe(move |&condition| {
            if condition {
                init_and_attach(&lazy1, &weak);
                Lazy::get(&lazy2).map(|child| child.html().set_hidden(true));
            } else {
                init_and_attach(&lazy2, &weak);
                Lazy::get(&lazy1).map(|child| child.html().set_hidden(true));
            }
        });

        self.push_dependency(unsub.droppable());
        self
    }
}

/// Weak reference to a component.
/// 
/// Does not prevent the component it refers to from being dropped.
/// 
/// # Examples
/// 
/// ```no_run
/// use wasmide::prelude::*;
/// 
/// let body = Component::root(Style::NONE);
/// let weak = body.downgrade();
/// ```
#[derive(Clone)]
pub struct WeakComponent(Weak<InternalComponent>);

impl WeakComponent {
    /// Tries to get a strong reference to the component. Will return `None` if the component
    /// has been dropped, or `Some(Component)` if it was not.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// use wasmide::prelude::*;
    /// 
    /// let body = Component::root(Style::NONE);
    /// let weak = body.downgrade();
    /// if let Some(body) = weak.upgrade() {
    ///     // do stuff...
    /// }
    /// ```
    pub fn upgrade(&self) -> Option<Component> {
        self.0.upgrade().map(Component)
    }
}