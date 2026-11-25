"""Graft parts of a donor PoB build into a target build (both as PoB XML).

Copies the donor's whole <Tree> (passives, masteries, ascendancy, socket->jewel map),
every jewel the donor has socketed in the tree (incl. cluster jewels), and optionally
donor items for named equipment slots (e.g. Amulet). Everything else (gear, gems,
config, level, bandit, pantheon) stays the target's.

Usage: python graft_build.py TARGET.xml DONOR.xml OUT.xml [--slot Amulet ...]
"""
import argparse
import re
import xml.etree.ElementTree as ET


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("target"); ap.add_argument("donor"); ap.add_argument("out")
    ap.add_argument("--slot", action="append", default=[], help="equipment slot to take from the donor")
    a = ap.parse_args()
    tgt, don = ET.parse(a.target), ET.parse(a.donor)
    troot, droot = tgt.getroot(), don.getroot()
    titems, ditems = troot.find("Items"), droot.find("Items")
    dmap = {it.get("id"): it for it in ditems.findall("Item")}
    next_id = max(int(it.get("id")) for it in titems.findall("Item")) + 1
    copied = {}

    def copy_item(did):
        nonlocal next_id
        if did in copied:
            return copied[did]
        src = dmap[did]
        new = ET.fromstring(ET.tostring(src))
        new.set("id", str(next_id))
        titems.append(new)
        copied[did] = str(next_id)
        next_id += 1
        return copied[did]

    # 1. tree (all specs); remap jewel socket item ids
    ttree, dtree = troot.find("Tree"), droot.find("Tree")
    new_tree = ET.fromstring(ET.tostring(dtree))
    for sock in new_tree.iter("Socket"):
        iid = sock.get("itemId")
        if iid and iid != "0":
            sock.set("itemId", copy_item(iid))
    troot.remove(ttree)
    troot.insert(list(troot).index(troot.find("Items")), new_tree)

    # 2. equipment slots from the donor's active item set
    def active_set(items):
        sid = items.get("activeItemSet", "1")
        for s in items.findall("ItemSet"):
            if s.get("id") == sid:
                return s
        return items.findall("ItemSet")[0]
    dset, tset = active_set(ditems), active_set(titems)
    for slot in a.slot:
        ds = [s for s in dset.findall("Slot") if s.get("name") == slot]
        ts = [s for s in tset.findall("Slot") if s.get("name") == slot]
        if not ds or ds[0].get("itemId", "0") == "0":
            raise SystemExit(f"donor has nothing in slot {slot!r}")
        nid = copy_item(ds[0].get("itemId"))
        if ts:
            ts[0].set("itemId", nid)
        else:
            ET.SubElement(tset, "Slot", {"name": slot, "itemId": nid})
    tgt.write(a.out, encoding="UTF-8", xml_declaration=True)
    print(f"wrote {a.out}: copied {len(copied)} donor item(s); slots {a.slot or '-'}")


if __name__ == "__main__":
    main()
