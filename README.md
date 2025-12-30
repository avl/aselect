# Safe Select

This is a rust crate intended to provide a solution to two potential pitfalls of the
tokio `select!`-macro. While said macro is useful, it has two slightly error-prone
characteristics, especially when used in loops:

 * Each iteration of the loop will cancel all but one future.
 * Handlers with async blocks may starve all select arms.

This crate solves this by:

 * Never cancelling futures (except when explicitly asked to)
 * Allowing the object created to be polled multiple times
 * Not allowing async code in handler blocks.

## Example

```rust
#[tokio::main]
async fn main() {
    let counter = 0u32;
    let mut stream = pin!(aselect!(
        {
            mutable(counter);
        },
        timer(
            {
                tokio::time::sleep(std::time::Duration::from_millis(1))
            },
            async |fut1| {
                fut1.await;
            },
            |result| {
                *counter += 1;
                None
            }
        ),
        output(
            {
                tokio::time::sleep(std::time::Duration::from_millis(3))
            },
            async |fut2| {
                fut2.await;
            },
            |result| {
                Some(*counter)
            }
        ),
    ));
    while let Some(item) = stream.next().await {
        println!("Value: {}", item);
    }
}

```

See docs and `examples/` for more complex examples. Especially, the above simple
example does not show more advanced state tracking (for example, `constant` and `borrowed` captures).
This example also doesn't show canceling.

# Philosophy of this crate

Being able to easily cancel futures is a useful feature of Rust. As is the ability to have
explicit control over the execution of async programs, using Rust's extendable and programmable
async features.

However, there are a few patterns that have turned out to be error-prone:
  1. Having futures that exist, but are not polled
  2. Canceling futures

The async programming model allows applications to be written mostly as if they were
normal sequential programs. However, this abstraction can be quite leaky. Point 1 above
means that a program can deadlock, if a future that owns a lock is not polled. Point 2 means
that async methods may not behave as expected.

A good introduction to the first problem can be found here:
<https://rfd.shared.oxide.computer/rfd/0609> (TLDR, if future that is not being polled
owns a resource, that resource will never become available).

An illustration of the other problem is something like this:

```rust
    async fn receive_command(&mut self) -> (Command, UserInfo) {
        let cmd = self.command_link.receive().await;
        self.security_log.send(cmd).await;
        let user_info = self.db.lookup_user_info(cmd.user).await;
        (cmd, user_info)
    }
```
This method receives a command, logs it in a security log, then looks up some additional
information and returns this to the caller. Consider what happens if this `receive_command` 
method is used as a future in regular tokio `select!`. Things may appear to work, but if
another future becomes ready after a command has been received and logged, but before 
`lookup_user_info` completes, the command will be silently dropped. In this case,
the command will still appear in the `security_log`, it will just not have any other effect.

Troubleshooting this kind of problem can be frustrating. Basically, any application that
uses tokio `select!` needs to annotate all its method with a `# Cancel safety` header
in its documentation, and all code must be inspected to see if cancel safety 
invariants are honored or not.

Note that this analysis must be done globally. It is typically hard to make all code
`Cancel safe`. Thus, one must ensure that async code in one module isn't called from a 
sometimes-canceled future elsewhere in the application.

Surely there is a better way!

# A better way

This crate suggests that futures should never be canceled, except potentially during
shutdown or as a response to faults. `aselect!` is a macro that allows writing
select loops that never cancel futures.











