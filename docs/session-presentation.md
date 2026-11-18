# Session presentation

The lock, login and authorization clients share `SessionScreen`. Public copy
scrolls within a card whose natural height is capped at the existing 460-pixel
authentication token and the available output height. The response field and
action row remain outside the scroll viewport. Short forms center at their
actual height.

Outputs shorter than the normal card plus its margins use existing compact
spacing tokens and omit the decorative LUCENT label. At 320×240, the card allows up to
96 logical pixels for public copy while retaining the 48-pixel field and
40-pixel actions. Titles, identity, authorization reasons, prompt labels and
status remain public presentation. Long public blocks use the toolkit's
per-line RichText layout, without links, with a generous 128 rendered-line bound
per block. Soft wrapping counts toward that bound; additional text ends with an
ellipsis. The known application title keeps its two-line bound.

Wheel, thumb dragging and keyboard scrolling use the existing controlled scroll
widget. Tab and Shift+Tab move between public copy and the current controls;
Home/End and Page Up/Down scroll when the public viewport has focus. Beginning an
attempt, receiving a prompt or result, and accepting a submitted response reset
the public viewport. Its callback and element identity both carry a presentation
generation, so queued scroll messages and an old thumb gesture cannot scroll a
new prompt. Typing and wallpaper changes preserve the current public offset.

Connecting and verifying fields have no input hit. Keyboard navigation still
reaches authorization Cancel, including Enter or Space activation. Escape keeps
its existing meaning, and an idle lock can still begin a new attempt with
Enter. Prompt delivery, subscription identities, backend authority and secure
session-lock roles are unchanged.

Secret prompts keep zeroizing response storage outside element and scene text;
their input control receives only the existing character count. A backend that
explicitly requests a Visible prompt still receives the deliberate echoed input.
The secure clients continue to disable clipboard transfer, input methods and
native keyboard-focus requests.

The source tests exercise actual toolkit layout and event routing across all
three modes, short outputs, long public tails, callback retirement and secret
presentation. Synthetic Vulkan fixtures cover compact initial, long and final
scroll positions plus connecting/verifying authorization. This slice adds no
SDK API; the independent `scroll-list` consumer already exercises capped natural
scroll sizing. See the [source-specific report](../reports/measurements/lucent-session-presentation-polish.json)
for executed checks. Hosted behavior checks pass; final visual comparison and
focus-fixture rendering remain pending. Native VM interaction remains deferred under the
user's instruction to keep the VM stopped.
