# Adobe Lightroom Classic Metadata Synchronization Guide

This guide describes how `photohelper` integrates with Adobe Lightroom Classic, how to sync metadata between the tools, and how to configure custom color labels.

---

## 1. How Lightroom Classic Syncs Metadata

Lightroom Classic uses a **passive, catalog-centric** database model. It does not automatically read or write XMP sidecars in real time unless configured to do so. This means:
* When `photohelper` writes or updates an `.xmp` sidecar, Lightroom Classic **will not show** the changes immediately.
* You must explicitly instruct Lightroom Classic to **reload** the metadata from disk to reflect rating, label, or keyword updates.

---

## 2. Triggering a Metadata Reload in Lightroom Classic

To sync `photohelper` classifications (ratings, labels, keywords) into Lightroom:

1. **Select Photos**: In Lightroom's **Library** module, select the photos you developed using `photohelper`.
2. **Read Metadata from File**:
   * Go to the top menu: **Metadata** > **Read Metadata from Files**.
   * Alternatively, right-click the selected photos and choose **Metadata** > **Read Metadata from Files**.
3. **Verify Changes**: After Lightroom reads the `.xmp` sidecars, you will see your ratings (1-5 stars), color labels, and keywords update instantly.

> [!WARNING]
> Selecting **Read Metadata from Files** will overwrite any un-saved changes you made in Lightroom with the values currently on disk. If you made edits in Lightroom that you want to keep, make sure to save them first (**Metadata** > **Save Metadata to Files** or `Cmd+S` / `Ctrl+S`).

---

## 3. Custom Lightroom Color Labels

Lightroom Classic uses localized strings for its color labels. For example, if Lightroom is configured for Portuguese, it expects `"Vermelho"` and `"Verde"` instead of `"Red"` and `"Green"`.

If the sidecar's written label does not exactly match your Lightroom's configured label translation, the color label will appear as white or gray text rather than painting the image frame with the physical color.

### CLI Configuration
You can pass custom color labels directly via the command line when running `develop`:
```bash
photohelper develop --lr-label --lr-label-red "Vermelho" --lr-label-green "Verde"
```

### Environment Variables
For convenience, you can configure these globally in your shell profile (`.zshrc` / `.bashrc`) using environment variables:
```bash
export PHOTOHELPER_LR_LABEL_RED="Vermelho"
export PHOTOHELPER_LR_LABEL_GREEN="Verde"
```

Once set, `photohelper develop` will automatically pick these up without requiring the CLI arguments.

---

## 4. Sorting by Aesthetic Value (NIMA Score)

Lightroom Classic does not have a native numeric "score" field. However, you can configure `photohelper` to inject the exact aesthetic NIMA score (e.g., `09.50`) into the color label field using the `--lr-label-score` flag:
```bash
photohelper develop --lr-label-score
```
*Note: This flag is mutually exclusive with `--lr-label` (color labels).*

### How to sort by score in Lightroom:
1. Ensure you have read the metadata from files (see Section 2).
2. Go to the **Grid View** (`G` key).
3. In the bottom toolbar, find the **Sort:** dropdown.
4. Select **Label Text** or **Label Color** (depending on your Lightroom version).
5. Ensure the sorting order is set to **Z to A** (Descending) to see the highest-scored photos first.

---

## 5. Auto Tone (Auto Enhance)

You can instruct Lightroom to automatically enhance your photos (applying its internal `AutoTone` engine) upon reading the metadata.
Use the `--auto-tone` flag:
```bash
photohelper develop --auto-tone
```

*Note: `photohelper` does not compute its own image enhancements; it simply writes the `crs:AutoTone="True"` directive, which triggers Lightroom's native algorithms.*

---

## 6. Conflict Protection & Shielding

To protect your manual Lightroom Classic adjustments, `photohelper` features an automatic filesystem-based **Conflict Shield**:
1. When you edit metadata or raw adjustments inside Lightroom Classic and save them to disk (using `Cmd+S` / `Ctrl+S`), Lightroom updates the sidecar file's physical modification time (`mtime`).
2. If `photohelper develop` detects that the sidecar file's `mtime` is **newer** than `photohelper`'s last write time (with a 2-second safety margin), it will **skip** writing to that photo to prevent overwriting your edits.
3. Skipped files are reported as `conflict-preserved` in the final summary.

### Overriding Conflicts
If you want to unconditionally force `photohelper` to overwrite manual Lightroom edits and apply its own ratings/labels/keywords, pass the `--force` flag:
```bash
photohelper develop --all-lr --force --auto-tone --lr-label-score
```
