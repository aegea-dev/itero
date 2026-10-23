# order

An order: its payment and stock checked side by side, the payment
waiting for its provider's message, then shipping and a mail. The
actions only pretend; each says what a real one would do. Run `itero
daemon` in this folder, then:
  itero emit order.placed '{"order": 43, "email": "ayse@example.com"}'
  itero emit payment.received '{"payment_ref": "pay-43", "paid": true}'

## Tests

`test/` has four tests: the order waits for its payment, ships once paid, fails
when the payment is refused, and is not reached by another order's
payment. Each test runs on a throwaway copy of this folder,
with `itero` from your `PATH` (or `$ITERO_BIN`). They need Node:

```sh
npm install
npm test
```
