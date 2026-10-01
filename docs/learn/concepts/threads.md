# threads

A thread is a second line of execution in the same program: the same memory, running at the same time as the first. `thread::spawn` starts one with a closure to run, and comes back at once, without waiting for it.

```rust
let handle = thread::spawn(move || {
    for i in 1..=3 {
        println!("[{:>3}ms]                  new thread {i}", ms());
        thread::sleep(Duration::from_millis(30));
    }
});
println!("[{:>3}ms] spawn came back (without waiting for the new thread)", ms());
for i in 1..=3 {
    println!("[{:>3}ms] main {i}", ms());
    thread::sleep(Duration::from_millis(30));
}
handle.join().unwrap();
```
```
[  0ms] spawn came back (without waiting for the new thread)
[  0ms] main 1
[  0ms]                  new thread 1
[ 30ms]                  new thread 2
[ 30ms] main 2
[ 61ms] main 3
[ 61ms]                  new thread 3
[ 91ms] done
```

Which of the two prints first at 30 ms is up to the system, and changes from run to run. `join` waits for the thread to end; when `main` returns, the whole process ends, threads and all.

## Why M9 needed one

The loop waited in one place, `event::read()`, which only a key, the mouse or a resize ends ([[event-loop]]). To hear that a file changed as well, it would have had to wait in two places at once, and one line of execution waits in one. So the terminal is read on a thread of its own, notify hears from the system on a thread of its own, and both hand what they get over one channel, which the loop waits on ([[channels]]).

## A thread may outlive the function that started it: `move`

`thread::spawn` takes only a closure that is `'static` — one that may still be running after `run` has returned, so it cannot borrow `run`'s variables ([[lifetimes]]). `move` makes a closure take what it uses instead of borrowing it ([[ownership]]). notify asks the same of the closure it calls from its thread, and in slice 2's code, without `move`:

```
error[E0373]: closure may outlive the current function, but it borrows `to_loop`, which is owned by the current function
    |     let mut watcher = notify::recommended_watcher(|_| {
    |                                                   ^^^ may outlive borrowed value `to_loop`
    |         let _ = to_loop.send(Message::Changed);
    |                 ------- `to_loop` is borrowed here
note: function requires argument type to outlive `'static`
help: to force the closure to take ownership of `to_loop` (and any other referenced variables), use the `move` keyword
```

The input thread's closure, `|| read_input(event::read, input, gone_on)`, compiles with or without `move`: it hands `input` and `gone_on` on to `read_input` by value, so it has to take them either way ([[closures]]). There `move` only says so.

## Where a thread is, and when

A thread goes through its code in order and waits where its code waits. The thread reading the terminal:

```rust
loop {
    if to_loop.send(Message::Input(read())).is_err() {   // waits in read() for an event
        return;
    }
    if go_on.recv().is_err() {                           // then for the loop to say go on
        return;
    }
}
```

Logged from both threads while the real screen ran — ① the input thread, ③ the loop, in milliseconds:

```
64000 ①  waiting in read()
64610 ①  read() returned: 'e' pressed -> sent
64612 ①  waiting in go_on.recv()
64612 ③  editor started
          … 1.58 seconds …
66190 ③  editor ended
66192 ③  go_on sent
66192 ①  go_on received
66193 ①  waiting in read()
```

## Two readers of one terminal

The editor reads the same terminal. A thread of ours still waiting in `read()` while it runs takes some of its keys. Driven in this console with "hello" typed into the editor, the editor got all of it in 5 runs of 20. In 7 the thread took the `e` of `hello`, so the loop opened the editor a second time, and the `q` typed afterwards went to that editor instead of the screen, which never ended. With the thread waiting in `go_on.recv()` until the loop is done with the editor, 20 runs in 20.

A version that sent `go_on` as soon as an event arrived, before handling it, also came through 10 in 10 — by accident. Leaving the alternate screen for the editor makes the Windows console report a resize; the thread read that and then waited for `go_on`, which the loop sends only once it has handled the resize, after the editor. Whether a terminal elsewhere reports a resize there was not checked.

## Ending

Nothing stops a thread from outside. The input thread returns when `send` fails or `recv` fails, and each means the other end of its channel is gone.

## Pitfalls hit

- **A thread taken to be where its work is.** Asked where the input thread is while the editor is open, the answer was *in `read()`, waiting for a key* — its job, rather than the line it had got to. The log above showed it in `go_on.recv()`. Two review questions about other moments — nothing pressed, and the loop handling `j` — were both answered right. Settled in slice 2.

## Related

[[channels]] · [[closures]] · [[ownership]] · [[lifetimes]] · [[event-loop]] · [[processes]]
