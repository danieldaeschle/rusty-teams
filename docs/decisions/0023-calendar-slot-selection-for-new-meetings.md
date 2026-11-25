---
status: proposed
date: 2026-11-25
---

# ADR 0023: Picking empty calendar slots for a new meeting

## Context

Teams creates a meeting from the calendar grid: select a time range by dragging over empty slots, then New meeting, and the form opens on that range. Microsoft's help only says "Select a timeslot on the calendar or select New". The drag and right-click flow comes from the Teams desktop client. A few details had more than one reasonable answer:

- what releasing the drag does: open the form at once, or keep the block for a right-click menu;
- whether a block may run across days;
- how a block that ends at midnight goes into the form;
- which clock a dragged row stands for when the grid draws a day across a daylight saving switch with today's offset.

## Decision

- Releasing the drag keeps the block. Right-click on it shows New meeting. Right-click on an empty slot outside it picks that one slot first. Double-click on an empty slot opens New meeting at once, for the block it lands in or for that slot.
- A block snaps to the time scale and stays in the day the drag started. The New meeting form has one date.
- A block that ends at 24:00 goes into the form as 23:59 and is labelled that way.
- The form gets the wall-clock times of the instants the dragged rows stand for on the grid (grid midnight plus minutes, converted to local time), so the meeting lands where it was dragged, also on the far side of a daylight saving switch.
- The block goes away on Escape, when New meeting opens, when its day leaves the view, in Month view, and when the time scale changes.

## Consequences

- The picking logic is plain functions in `app-call-model::slot_select`, unit tested apart from the view.
- No multi-day blocks and no auto-scroll while dragging past the grid edge. Picking a slot needs a mouse; the New meeting button stays the keyboard path.
- The grid itself still lays every shown day out with today's offset. Fixing that is a separate change, and `SlotSelection::form_times` must then take the same per-day origin as the grid.
