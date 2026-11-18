# Wallpaper browsing and selection

Open **Wallpapers** with Super+Ctrl+Space. Local images, Wallhaven and Alpha
Coders use the existing native selector. **Live** opens the separate local-video
picker described in [live wallpapers](live-wallpapers.md). Local and live
libraries share the desktop carousel; online results use native cards and the
same design tokens.

## Search and paging

Editing the online search field changes a draft. **Search** submits that draft
from the first page. The page arrows continue the last submitted search and
provider, even if the field now contains an unsubmitted edit. For example,
searching for `mountains`, typing `ocean` and then choosing Next requests the next
page of `mountains`; pressing Search starts `ocean` at page one.

Enter retains the desktop's existing distinction between submitting a changed
query and applying the selected result once the query is submitted. Ordinary
selection, clipboard and undo use the shared text-field API. Typing alone does
not contact a provider.

Each result page belongs to a particular provider/search request. Rendered page
arrows and cards retain that result identity; receiving a later page or changing
provider cannot reinterpret an older callback as an action on the replacement
results. Thumbnail arrival does not turn the same page into a new selection.
The previous/next controls remain subject to actual page availability and the
current request's busy state. Starting a search clears the previous page bounds;
a response with a different page or provider is rejected instead of becoming
the active result set.

Search, page and download buttons show their enabled state from the same checks
that admit their actions. Unavailable buttons have no pointer or Tab action.
Result metadata uses one line and status text two, using generated component
tokens so narrow selectors retain reachable controls.

## Choosing and applying

Selecting a card changes the current choice. **Wallpaper only** and
**Wallpaper + colors** act on the selected item shown when the control was rendered. A new
selection, replacement result set or retired picker cannot redirect that
callback to another wallpaper. Returning from A to B to A requires a fresh
Apply action; matching the same final item does not revive an older action.
Accepting a download retires the captured Apply as well, so delivering it again
cannot start the same request after completion.

The root checks current child authority before reserving a wallpaper selection
or requesting work from an adapter. Rejected callbacks do not create an import,
download, preference save or unrelated writable-settings error. Accepted work
continues through the existing wallpaper transaction and persistence owners.
Provider/download failures preserve the active wallpaper; selecting a palette
during a download retains precedence over its automatic colors.

In the live picker, cards retain their source descriptor and library lifetime.
Apply also retains the selected source and target lifetime. Refresh, replacement
library results, close/reopen or a changed selection/target retire earlier
actions. Preview frames and viewport changes for the same source do not retire
otherwise current controls; selecting the same source again also preserves them.
Apply is unavailable while discovery or another import is pending. Browsing and
previewing remain separate from import and saved wallpaper selection.

Panel controls require their current open presentation. Global CLI wallpaper
commands retain their separate authority and continue to work while the panel
is closed. Closing the online selector retires its controls while an already
accepted download keeps its existing transaction lifetime. Closing the live
panel retires pending import delivery; already-running native preparation may
finish and retain an unused owned asset. Neither case revives a retired callback
or permits old work to replace a newer admitted wallpaper choice.

The browser receives only `WallpaperCatalogPort` and `AssetPort` for its service
subscriptions. It does not own the desktop adapter bundle. Shared text editing,
element mapping and design tokens remain reusable toolkit contracts; search,
selection and import authority belong to the desktop children and coordinator.

## Verification scope

Behavior regressions capture the actual SDK callbacks from rendered controls,
then change page, provider, selection, library or panel lifetime before
delivering them. Valid controls are also exercised across thumbnail/preview and
viewport updates so ordinary asynchronous presentation does not disable them.
Root integration checks distinguish rejected callbacks from admitted work and
preserve explicit global commands.
The authored tests include 320- and 640-pixel layouts, 240- and 580-pixel height
cases, both themes, exact injected provider calls and duplicate completions.

The [appearance source report](../reports/measurements/lucent-appearance-selection-polish.json)
records execution status. These source changes do not establish native
pointer/keyboard behavior, provider availability, compositor pixels, animation
pacing or resource acceptance. Earlier native wallpaper reports keep their own
source and workload scopes.
