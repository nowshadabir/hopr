#!/usr/bin/env python3
import argparse
import email.utils
import html
import os
import xml.etree.ElementTree as ET

SPARKLE_NS = "http://www.andymatuschak.org/xml-namespaces/sparkle"
DC_NS = "http://purl.org/dc/elements/1.1/"

ET.register_namespace("sparkle", SPARKLE_NS)
ET.register_namespace("dc", DC_NS)


def build_notes_html(version: str, notes_file: str | None) -> str:
    lines = []
    if notes_file and os.path.exists(notes_file):
        with open(notes_file, "r", encoding="utf-8") as f:
            for raw in f:
                s = raw.strip()
                if not s or s.startswith("#"):
                    continue
                if s.startswith("- ") or s.startswith("* "):
                    s = s[2:].strip()
                lines.append(f"<li>{html.escape(s)}</li>")
    if not lines:
        lines.append("<li>Performance, stability, and security improvements.</li>")

    items_html = "\n      ".join(lines[:10])
    return f"""<div style="font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Text', sans-serif; font-size: 13px; line-height: 1.55; color: #1c1e1c;">
  <h3 style="margin: 0 0 8px; font-size: 15px; color: #0e5a46;">What's New in Hopr {html.escape(version)}</h3>
  <ul style="margin: 0; padding-left: 18px;">
      {items_html}
  </ul>
</div>"""


def main() -> None:
    parser = argparse.ArgumentParser(description="Prepend a release item to Sparkle appcast.xml")
    parser.add_argument("--appcast", required=True)
    parser.add_argument("--version", required=True, help="CFBundleShortVersionString, e.g. 1.0.0")
    parser.add_argument("--build", required=True, help="Numeric CFBundleVersion, e.g. 10000")
    parser.add_argument("--min-os", default="14.0")
    parser.add_argument("--url", required=True)
    parser.add_argument("--ed-signature", required=True)
    parser.add_argument("--length", required=True)
    parser.add_argument("--notes-file", default=None)
    parser.add_argument("--release-notes-url", default=None)
    args = parser.parse_args()

    tree = ET.parse(args.appcast)
    root = tree.getroot()
    channel = root.find("channel")
    if channel is None:
        raise SystemExit("Invalid appcast.xml: missing <channel>")

    # Remove any existing item with the same build number to keep updates idempotent
    for existing in list(channel.findall("item")):
        ver_el = existing.find(f"{{{SPARKLE_NS}}}version")
        if ver_el is not None and ver_el.text == args.build:
            channel.remove(existing)

    item = ET.Element("item")
    ET.SubElement(item, "title").text = f"Hopr {args.version}"
    ET.SubElement(item, "pubDate").text = email.utils.formatdate(usegmt=True)
    ET.SubElement(item, "description").text = build_notes_html(args.version, args.notes_file)
    ET.SubElement(item, f"{{{SPARKLE_NS}}}version").text = args.build
    ET.SubElement(item, f"{{{SPARKLE_NS}}}shortVersionString").text = args.version
    ET.SubElement(item, f"{{{SPARKLE_NS}}}minimumSystemVersion").text = args.min_os

    enclosure = ET.SubElement(item, "enclosure")
    enclosure.set("url", args.url)
    enclosure.set(f"{{{SPARKLE_NS}}}edSignature", args.ed_signature)
    enclosure.set("length", args.length)
    enclosure.set("type", "application/octet-stream")

    insert_idx = len(channel)
    for i, child in enumerate(channel):
        if child.tag == "item":
            insert_idx = i
            break
    channel.insert(insert_idx, item)

    ET.indent(tree, space="  ")
    tree.write(args.appcast, encoding="utf-8", xml_declaration=True)


if __name__ == "__main__":
    main()
