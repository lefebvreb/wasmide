#!/usr/bin/env python

from contextlib import contextmanager
from dataclasses import dataclass
from re import sub
from typing import Optional

import pandas as pd
from requests import head

if __name__ != "__main__":
    exit(0)

# Links to the main MDN resources
MDN = "https://developer.mozilla.org"
MDN_ELEMENTS = f"{MDN}/en-US/docs/Web/HTML/Element"
MDN_ATTRIBUTES = f"{MDN}/en-US/docs/Web/HTML/Attributes"

# The header of the generated rust file.
HEADER = f"""// Programmatically generated by scripts/codegen.py, do not edit manually.

//! HTML elements and attributes definitions.
//! 
//! In the Wasmadeus framework, [HTML elements]({MDN_ELEMENTS}) are replaced
//! with simple rust functions taking one or more [`Attributes`]
//! for input and returning a [`Component`].
//! 
//! [HTML attributes]({MDN_ATTRIBUTES}) are simply rust structs that implement 
//! the [`Attribute`] trait.
//!
//! This module contains the definitions and documentation of all standard HTML 
//! elements and attributes, including the deprecated and experimental ones.

use web_sys::Element;

use crate::attribute::{{attributes, Attribute, Attributes}};
use crate::component::{{elements, Component}};
use crate::signal::Value;
use crate::util::TryAsRef;
"""

# Attributes are renamed from their HTML names by simply making the first
# letter in their name uppercase, except for the following attributes that
# are manually renamed:
ATTRIBUTE_RENAME_OVERRIDE = {
    "accept-charset": "AcceptCharset",
    "accesskey": "AccessKey",
    "autocapitalize": "AutoCapitalize",
    "autocomplete": "AutoComplete",
    "autofocus": "AutoFocus",
    "autoplay": "AutoPlay",
    "bgcolor": "BgColor",
    "contenteditable": "ContentEditable",
    "contextmenu": "ContextMenu",
    "crossorigin": "CrossOrigin",
    "datetime": "DateTime",
    "dirname": "DirName",
    "enctype": "EncType",
    "enterkeyhint": "EnterKeyHint",
    "formaction": "FormAction",
    "formenctype": "FormEnctype",
    "formmethod": "FormMethod",
    "formnovalidate": "FormNoValidate",
    "formtarget": "FormTarget",
    "hreflang": "HrefLang",
    "http-equiv": "HttpEquiv",
    "intrinsicsize": "IntrinsicSize",
    "inputmode": "InputMode",
    "ismap": "IsMap",
    "itemprop": "ItemProp",
    "maxlength": "MaxLength",
    "minlength": "MinLength",
    "novalidate": "NoValidate",
    "placeholder": "PlaceHolder",
    "playsinline": "PlaysInline",
    "readonly": "ReadOnly",
    "referrerpolicy": "ReferrerPolicy",
    "rowspan": "RowSpan",
    "sandbox": "SandBox",
    "spellcheck": "SpellCheck",
    "srcdoc": "SrcDoc",
    "srclang": "SrcLang",
    "srcset": "SrcSet",
    "tabindex": "TabIndex",
    "usemap": "UseMap",
    "value": "DefaultValue",
}

# =============================================================================

# Formats a link from a route on MDN, testing if the link is still valid.
def format_link(mdn_route: Optional[str]) -> str:
    MISSING = "*Missing MDN documentation.*"

    if mdn_route is None:
        return MISSING

    # Get HTTP header of the page and check that it is 200 OK.
    full_link = f"{MDN}{mdn_route}"
    res = head(full_link)

    # If any error occured, simply return a generic message instead of the link.
    if res.status_code == 200:
        return f"[MDN documentation.]({full_link})"
    else:
        return MISSING

# =============================================================================

# Scraped data about an element.
@dataclass
class Element:
    # Original HTML name in angle brackets: <element>
    name: str
    # Description string.
    desc: str
    # Is the element deprecated.
    deprecated: bool
    # A markdown link to the relevant MDN page, or an message if such page does not exist. 
    mdn_link: str
    # The name of the rust identifier that is used for the rust function that represents this
    # element in Wamsadeus.
    rust_name: str
    # The markdown link to this element's rust function.
    rust_link: str
    # A list of the attribute's rust links that may be applied to this element, empty at first.
    possible_attributes: list[str]

elements = {}

def make_element(name, mdn_route, desc, deprecated):
    link = format_link(mdn_route)
    rust_name = name[1:-1]
    rust_link = f"[`{rust_name}`]"

    global elements
    elements[name] = Element(
        name,
        desc,
        deprecated,
        link,
        rust_name,
        rust_link,
        [],
    )

def extract_element(row, deprecated):
    (name, mdn_route), (desc, _) = row

    if name.startswith("<h1>"):
        for i, name in enumerate(name.split(", ")):
            desc = f"Represents a section heading of level {i + 1}. <h1> being the highest and <h6> the lowest."
            make_element(name, mdn_route, desc, deprecated)
    else:
        make_element(name, mdn_route, desc, deprecated)


tables = pd.read_html(MDN_ELEMENTS, extract_links="all")
for table in tables[:-1]:
    table.apply(extract_element, axis="columns", deprecated=False)
tables[-1].apply(extract_element, axis="columns", deprecated=True)

# =============================================================================

@dataclass
class Attribute:
    name: str
    desc: str
    deprecated: bool
    mdn_link: str
    rust_name: str
    rust_link: str
    possible_elements: Optional[list[str]]
    content_editable: bool

attributes = {}

def extract_attribute(row):
    (name, mdn_route), (elements, _), (desc, _) = row

    name, *warnings = name.replace("  ", " ").split(" ")
    deprecated = any([warning.lower() == "deprecated" for warning in warnings])

    if name == "data-*":
        return

    link = format_link(mdn_route)

    if elements == "Global attribute":
        elements = None
    else:
        elements = list(elements.replace("  ", " ").split(", "))

    content_editable = elements is not None and "contenteditable" in elements
    if content_editable:
        elements.remove("contenteditable")

    if (override := ATTRIBUTE_RENAME_OVERRIDE.get(name)) is not None:
        rust_name = override
    else:
        rust_name = name[0].upper() + name[1:]

    rust_link = f"[`{rust_name}`]"

    if desc == "":
        desc = "*Missing MDN description.*"
    else:
        desc = desc.replace("  ", " ")

    global attributes
    attributes[name] = Attribute(
        name,
        desc,
        deprecated,
        link,
        rust_name,
        rust_link,
        elements,
        content_editable,
    )

tables = pd.read_html(MDN_ATTRIBUTES, extract_links="all")
tables[0].apply(extract_attribute, axis="columns")

# =============================================================================

# Make elements link to attributes.
for attr in attributes.values():
    if attr.possible_elements is None:
        # Can be applied to any element.
        for elem in elements.values():
            elem.possible_attributes.append(attr.name)
    else:
        # Can be applied to specific elements.
        for elem_name in attr.possible_elements:
            elements[elem_name].possible_attributes.append(attr.name)

# =============================================================================

# Sort possible elements and attributes alphabetically.
for attr in attributes.values():
    if attr.possible_elements is not None:
        attr.possible_elements = [elements[name].rust_link for name in attr.possible_elements]
        attr.possible_elements.sort()
for elem in elements.values():
    elem.possible_attributes = [attributes[name].rust_link for name in elem.possible_attributes]
    elem.possible_attributes.sort()

# =============================================================================

# Replace elements name in angles brackets by their rust links.
IN_ANGLED_BRACKETS = r"<(.+?)>"
REPLACE_FN = lambda x: f"{elements[x.group()].rust_link}"

for dic in [attributes, elements]:
    for name, obj in dic.items():
        if name == "manifest":
            # Special case for manifest atribute.
            obj.desc = obj.desc.replace("<link rel=\"manifest\">", "`<link rel=\"manifest\">`")
        else:
            obj.desc = sub(IN_ANGLED_BRACKETS, REPLACE_FN, obj.desc)
        
# =============================================================================

# Sort attributes and elements by name.
key = lambda x: x.rust_name
attributes = sorted(attributes.values(), key=key)
elements = sorted(elements.values(), key=key)

# =============================================================================

# For debugging
# for attr in attributes:
#     print(attr)
# for elem in elements:
#     print(elem)

# Opens a rust macro call.
@contextmanager
def macro_call(macro_name: str):
    print(macro_name + "! {")
    yield
    print("}")

def print_doc(doc: list[str]):
    for line in doc:
        print(f"    /// {line}")

# =============================================================================

print(HEADER)

with macro_call("attributes"):
    for attr in attributes:
        doc = [
            attr.desc,
            "",
            f"Corresponds to the HTML attribute: `{attr.name}`."
            "",
            "",
        ]
        if attr.possible_elements is not None:
            possible_elements = ", ".join(attr.possible_elements)
            doc.append(f"Can be applied to the following elements: {possible_elements}.")
        else:
            doc.append("Global attribute: can be applied to any HTML element.")
        doc.append("")
        doc.append(attr.mdn_link)
        print_doc(doc)

        if attr.deprecated:
            print("    #[deprecated = \"This HTML attribute is deprecated in the latest standard.\"]")
        print(f"    {attr.rust_name} => \"{attr.name}\",")

print()

with macro_call("elements"):
    for elem in elements:
        possible_attributes = ", ".join(elem.possible_attributes)
        doc = [
            elem.desc,
            "",
            f"Corresponds to the HTML element: `{elem.name}`.",
            "",
            f"Supports the following attributes: {possible_attributes}",
            "",
            elem.mdn_link
        ]
        print_doc(doc)

        if elem.deprecated:
            print("    #[deprecated = \"This HTML element is deprecated in the latest standard.\"]")
        print(f"    {elem.rust_name} => \"{elem.rust_name}\",")
