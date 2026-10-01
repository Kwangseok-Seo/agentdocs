# channels

A channel carries values from one thread to another. `mpsc::channel()` makes its two ends, a `Sender` and a `Receiver`:

```rust
let (to_loop, messages) = mpsc::channel();
```

- `send(value)` hands the value over. It belongs to the channel now, and the sender cannot use it afterwards ([[ownership]]). It does not wait: this kind of channel has no limit.
- `recv()` waits until something arrives. `recv_timeout(d)` waits at most `d`; `try_recv()` does not wait at all.
- **mpsc** is *multiple producer, single consumer*. A `Sender` can be cloned, one for each thread that sends; there is one `Receiver`.
- When every `Sender` is gone, `recv` answers `Err(Disconnected)` rather than waiting for ever. When the `Receiver` is gone, `send` answers `Err`.

Two threads sending — one after 50 ms, one after 150 — and one receiving with `recv_timeout` of 80 ms:

```
[ 50ms] received: a file changed
[143ms] nothing for 80 ms (Timeout)
[150ms] received: key j
[150ms] every sender is gone (Disconnected)
```

## One channel, two kinds of thing

A channel carries one type. The loop waits for two kinds of thing, so the type is an enum ([[enums-and-data]]):

```rust
enum Message {
    Input(io::Result<Event>),   // from the thread reading the terminal
    Changed,                    // from notify's: a file may have changed
}
```

and the loop waits in one place, where until M9 it called `event::poll`:

```rust
let waited = match app.wake_in(Instant::now()) {
    Some(wait) => messages.recv_timeout(wait),
    None => messages.recv().map_err(RecvTimeoutError::from),
};
```

`wake_in` is M8's: how long until something is due — now including the Sources being read again, 100 ms after a change ([[event-loop]]). `RecvError` turns into `RecvTimeoutError` through `From`, so both arms give one type.

## A second channel, going back

`go_on` carries `()`: no value, only *now*. The loop sends it once it is done with an event, an editor run from it included, and the thread reading the terminal reads nothing more until it arrives ([[threads]]).

## A function another thread calls

notify is not given a `Sender`. It is given something to call, from its own thread, with each thing it reports, and `tell` builds that around the `Sender` it is handed:

```rust
fn tell(to_loop: Sender<Message>) -> impl FnMut(notify::Result<notify::Event>) + Send + 'static {
    move |event| {
        if !matches!(&event, Ok(e) if e.kind.is_access()) {
            let _ = to_loop.send(Message::Changed);
        }
    }
}
```

`Send` promises the closure may be moved to another thread; `'static`, that it borrows nothing that could end first ([[lifetimes]]). The closure decides which reports are worth a message: a file opened, read or closed is not a change ([ADR-0011](../../adr/0011-changes-are-heard-not-polled.md)).

## Pitfalls hit

- **One `Sender`, moved twice.** Asked how the thread reading the terminal gets a `Sender` while notify's keeps its own, the answer was `let input = to_loop;`. That moves it, and the compiler said so:

  ```
  error[E0382]: use of moved value: `to_loop`
  967 |     let (to_loop, messages) = mpsc::channel();
      |          ------- move occurs because `to_loop` has type `std::sync::mpsc::Sender<Message>`, which does not implement the `Copy` trait
  970 |     let input = to_loop;
      |                 ------- value moved here
  973 |     let mut watcher = notify::recommended_watcher(move |_| {
      |                                                   ^^^^^^^^ value used here after move
  help: consider cloning the value if the performance cost is acceptable
  970 |     let input = to_loop.clone();
  ```

  The explanation before the quiz had covered `move` and `clone` one at a time, and never the two together. A review question in another shape — one `tx` moved into two threads — was answered right: E0382 again. Settled in slice 2.

## Related

[[threads]] · [[ownership]] · [[enums-and-data]] · [[event-loop]] · [[closures]]
