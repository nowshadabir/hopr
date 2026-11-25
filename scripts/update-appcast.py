#!/usr/bin/env python3
import argparse
import email.utils
import xml.etree.ElementTree as ET

SPARKLE_NS = "http://www.andymatuschak.org/xml-namespaces/sparkle"
DC_NS = "http://purl.org/dc/elements/1.1/"

ET.register_namespace("sparkle", SPARKLE_NS)
ET.register_namespace("dc", DC_NS)


def main() -> None:
    parser = argparse.ArgumentParser(description="Prepend a release item to Sparkle appcast.xml")
    parser.add_argument("--appcast", required=True)
    parser.add_argument("--version", required=True, help="CFBundleShortVersionString, e.g. 1.0.0")
    parser.add_argument("--build", required=True, help="Numeric CFBundleVersion, e.g. 10000")
    parser.add_argument("--min-os", default="14.0")
    parser.add_argument("--url", required=True)
    parser.add_argument("--ed-signature", required=True)
    parser.add_argument("--length", required=True)
    parser.add_argument("--release-notes-url", required=True)
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
    ET.SubElement(item, f"{{{SPARKLE_NS}}}version").text = args.build
    ET.SubElement(item, f"{{{SPARKLE_NS}}}shortVersionString").text = args.version
    ET.SubElement(item, f"{{{SPARKLE_NS}}}minimumSystemVersion").text = args.min_os
    ET.SubElement(item, f"{{{SPARKLE_NS}}}releaseNotesLink").text = args.release_notes_url

    enclosure = ET.SubElement(item, "enclosure")
    enclosure.set("url", args.url)
    enclosure.set(f"{{{SPARKLE_NS}}}edSignature", args.ed_signature)
    enclosure.set("length", args.length)
    enclosure.set("type", "application/octet-stream")

    # Insert right after channel metadata (before older <item> elements)
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
