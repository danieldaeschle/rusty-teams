## 1. Floating meeting card

- [x] 1.1 While a desktop notification service is connected, open meeting reminder cards as well as ring cards.
- [x] 1.2 Meeting cards use the same window options as the ring card.
- [x] 1.3 Chat toasts still do not open a card in that mode.

## 2. Verify

- [x] 2.1 `cargo test -p app --bin teams call_cards_while_desktop`
- [x] 2.2 `openspec validate linux-meeting-card`
