# Controlled list navigation and scrolling

`lucent-api` exposes `ListNavigation` and `list_window` for ordinary selectors.
They contain no item model, application IDs, native objects, effects or rendering
state. The client retains its items, stable identities, selection and previous
window; it supplies the actual number of visible rows and a selectable predicate.

`ListNavigation::target` returns an optional selectable index. Arrow navigation
skips disabled rows; pages count visible rows, including disabled rows, and choose
an enabled row near the destination. Endpoints clamp instead of wrapping. Empty
or entirely disabled lists have no destination. `list_window` returns a bounded
range that reveals the selection while retaining the previous window where
possible. Both functions handle stale indices and zero-sized viewports.

```rust,ignore
if let Some(navigation) = ListNavigation::from_key(&key) {
    if let Some(next) = navigation.target(selected, items.len(), visible_rows,
                                         |index| !items[index].disabled) {
        selected = next;
        first = list_window(selected, first, items.len(), visible_rows).start;
    }
}
```

This is an API excerpt. The client chooses which keys belong to its list.
The launcher and native picker use Page Up/Down while leaving Home/End with
ordinary search-field caret movement. Launcher sections without an editor and
application-action lists also support Home/End. Modified editor keys stay with
the editor; the API does not strip modifiers or manufacture native focus.

## Existing toolkit scroll controls

The launcher and picker render their row windows inside `Element::scroll`, with
controlled `ScrollSpec` offsets and the real content height. Rows use original
content coordinates; invisible rows need no element or prepared assets. The
viewport contains whole rows, and wheel/track/thumb events snap to a row boundary.
The selected row retains its relative visible position when the viewport moves.
No new selection, scrollbar painting or pointer capture is added to the renderer.

`Theme::scroll_style()` supplies the same thumb, gutter and minimum-thumb tokens
as the existing padded `Theme::scroll` recipe. The launcher reserves one shell
grid step for that gutter; the picker uses the standard component gutter. Search
focus is retained while the thumb is dragged.

## Identity and lifetime belong to the client

Launcher search results retain `AppId` values rather than indices into a mutable
catalog snapshot. Rendered rows use those IDs, and hover resolves the current
rank from the captured ID. A catalog update cannot turn an old press into a
new application's row. The picker likewise uses immutable source-entry indices,
then resolves the current filtered rank before hover or acceptance. It never
accepts a replacement rank-zero entry through a captured old row.

Replacing the result set changes the scroll-control identity and retires a
captured thumb, including when the new list has the same count and dimensions.
Callbacks also carry the current result/query revision. The generic runtime
keeps its existing native-surface and capture lifetimes.

App search text, selected AppId and scroll survive section changes while the
launcher remains open. Catalog refresh reconciles that saved identity without
changing the active section's selection. Closing retires pending clipboard
work; a fresh open starts a clean query. These transient values are not saved
preferences and introduce no extra settings writer.

## Verification scope

The launcher feature is in progress. API, launcher and independent picker
behavior tests cover paging, disabled/empty results, result replacement, retained
section context and stale callbacks. The real layout test checks the toolkit
scroll bounds and retires a held thumb after a replacement with identical
geometry. Native VM input, visual baselines and protected-main delivery are
separate gates in [the polish backlog](desktop-polish.json).
