# Part 1 - Async application main loop pitfalls

## Background

This document takes a look at an async rust application and illustrates a few pitfalls. In [part 2](EXAMPLE.md),
we take a look at how these challenges can be overcome using the "aselect" crate.

This article is intended for developers with some experience of rust async. I know it's a bit long,
but I'm trying to make sure that I state clearly the problems I'm trying to illustrate.

## Scenario

Let's say you're writing a server for an embedded system. The system is controlled through a custom
protocol built on top of TCP. The hardware has a temperature sensor and a heater. A client should be able
to read the temperature and control the heater.

Let's start by writing a simple server. We'll assume there's only a single client at a time.

### A starting point

An initial minimal main loop might look something like this:

```rust
use tokio::net::TcpStream;
use tokio::io::AsyncReadExt;
use std::io::Result;

async fn set_heater_power(power: u8) { /* implementation */ }

async fn run_server(stream: &mut TcpStream) -> Result<()> {
    let (mut reader, writer) = stream.split();
    
    loop {

        let cmd: u8 = reader.read_u8().await?;
        match cmd {
            1 => {
                let power: u8 = reader.read_u8().await?;
                set_heater_power(power).await;
            }
            _ => {
                // Unknown command: Do error handling
            }
        }
    }
}
```

The above example works reliably. Clients write a 1-byte command to adjust power, followed by a 1-byte parameter with 
the desired power. While `set_heater_power` is executing, no further commands are processed. However,
this is acceptable for now.

### Adding a query feature

Now, let's add a way to query the current temperature:

```rust

async fn set_heater_power(power: u8) {}
async fn measure_temperature() -> u8 {42}

use tokio::net::TcpStream;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use std::io::Result;

async fn run_server(stream: &mut TcpStream) -> Result<()> {
    let (mut reader, mut writer) = stream.split();
    
    loop {

        let cmd: u8 = reader.read_u8().await?;
        match cmd {
            1 => {
                let power: u8 = reader.read_u8().await?;
                set_heater_power(power).await;
            }
            2 => { // Query temperature
                let temperature = measure_temperature().await;
                writer.write_u8(2).await?;
                writer.write_u8(temperature).await?;
            }
            _ => {
                // Unknown command: Do error handling
            }
        }
    }
}
```

This adds a second type of command, encoded as a byte with value '2'. Upon receiving such a command, the server
writes a magic byte (2) back, followed by the temperature encoded in a byte. Let's not worry about units or 
data types for physical quantities for this example.

### Adding an alarm feature

Now, let's add an alarm feature. Whenever the temperature exceeds 100, the client should be notified immediately,
without having to perform a request. To achieve this, we need a primitive that allows us to monitor two different
futures for completion: The temperature (from the hardware itself), and incoming requests (from the client).

Tokio provides such a primitive, `tokio::select`:

Our program might now looks something like this:

```rust

async fn set_heater_power(power: u8) {}
async fn measure_temperature() -> u8 {42}
/// Waits for the temperature to change, then returns the new value
async fn wait_temperature_alarm() -> u8 {42}

use tokio::net::TcpStream;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use std::io::Result;
use tokio::select;

async fn run_server(stream: &mut TcpStream) -> Result<()> {
    let (mut reader, mut writer) = stream.split();
    
    loop {
        select!{
            cmd = reader.read_u8() => {
                match cmd? {
                    1 => {
                        let power: u8 = reader.read_u8().await?;
                        set_heater_power(power).await;
                    }
                    2 => { // Query temperature
                        let temperature = measure_temperature().await;
                        writer.write_u8(2).await?;
                        writer.write_u8(temperature).await?;
                    }
                    _ => {
                        // Unknown command: Do error handling
                    }
                }                 
            },
            new_temperature = wait_temperature_alarm() => {
                writer.write_u8(2).await?;
                writer.write_u8(new_temperature).await?;
            }
        }
    }
}
```

#### A potential bug

The above program is likely to work well in practice, but it potentially has a subtle bug: If `wait_temperature_alarm`
completes frequently, it may end up saturating the TcpStream send buffer, effectively blocking on `writer.write_u8`.
If the client has somehow managed to fill up its send-queue too, the system will deadlock. This may be quite unlikely
to happen for this simple example, but as a system grows more complex, this type of issue will be more likely.

A similar potential misfeature is that while `set_heater_power` is executing, the alarm feature is not active.

Both these limitations may be perfectly fine, depending on circumstances such as buffer sizes and client behavior. 

### Refactoring

Now, let's leave these concerns for a while and consider cleaning up the program a little. Mixing protocol parsing
and logic like this can make the program harder to reason about. So let's abstract the protocol parsing into
separate functions:


```rust

async fn set_heater_power(power: u8) {}
async fn measure_temperature() -> u8 {42}
/// Waits for the temperature to change, then returns the new value
async fn wait_temperature_alarm() -> u8 {42}

use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp;
use std::io::Result;
use tokio::select;

enum Command {
    SetPower(u8),
    QueryTemperature,
}
enum Response {
    Temperature(u8)
}

async fn read_command(reader: &mut tcp::ReadHalf<'_>) -> std::io::Result<Command> {
    Ok(match reader.read_u8().await? {
        1 => {
            Command::SetPower(reader.read_u8().await?)
        }
        2 => { // Query temperature
            Command::QueryTemperature
        }
        _ => panic!("unexpected command")        
    })
}

async fn write_response(writer: &mut tcp::WriteHalf<'_>, response: Response) -> std::io::Result<()> {
    match response {
        Response::Temperature(temperature) => {
            writer.write_u8(2).await?;
            writer.write_u8(temperature).await?;
        }
    }
    Ok(())
}

async fn run_server(stream: &mut TcpStream) -> Result<()> {
    let (mut reader, mut writer) = stream.split();
    
    loop {
        select!{
            cmd = read_command(&mut reader) => {
                match cmd? {
                    Command::SetPower(power) => {
                        set_heater_power(power).await;
                    }
                    Command::QueryTemperature => { // Query temperature
                        let temperature = measure_temperature().await;
                        write_response(&mut writer, Response::Temperature(temperature)).await?;
                    }
                }                 
            },
            new_temperature = wait_temperature_alarm() => {
                write_response(&mut writer, Response::Temperature(new_temperature)).await?;
            }
        }
    }
}
```

Nice! Protocol parsing is no longer intertwined with program logic.

At first glance, the code looks correct, but it is broken.

If the temperature changes while a SetPower command is being read, the future returned by `read_command` 
will be canceled. But it may already have consumed the first byte of the 2-byte on-wire packet. 
In the next iteration of the loop, the 'power' parameter will now be interpreted as a new command.

The cause of this framing error is that `read_command` is not "cancel safe". For an excellent
article on cancel safety, see: <https://rfd.shared.oxide.computer/rfd/400>.

### Canceling temperature reading 

Let's continue this slightly contrived journey, and look at the method `wait_temperature_alarm`.
Imagine that its innards perform hardware operations like this:

```rust
use tokio::time::Duration;
use tokio::time::sleep;
fn enable_measuring_current() {}
fn sample_ad_converter() -> u8 {42}
async fn wait_temperature_alarm() -> u8 {
    loop {
        sleep(Duration::from_millis(1000)).await;
        
        enable_measuring_current();
        sleep(Duration::from_millis(1)).await;
        let temperature = sample_ad_converter();
        if temperature > 100 {
            return temperature;
        }
    }
}
```

Imagine this is measuring some very sensitive chemical process, or whatever, sending current through at PT100 
resistive temperature probe. A precise current is enabled, and the voltage drop across the actual temperature sensing 
element is measured. However, we only want to enable the current while actually measuring, because the current will 
make the probe generate heat, affecting the precision of the measurement.

Now, we notice that it's not great if the above method is canceled. It could result in the current being enabled
while `write_response` is running. If the network is slow, or the client fails to read, or similar, this could
leave the current enabled for an unbounded amount of time. Depending on the chemical process measured, this could 
conceivably be harmless or catastrophic (I'm not a chemist - maybe you can already tell).

To avoid the possibility of this unwanted cancellation, let's modify the main program like this:


```rust

async fn set_heater_power(power: u8) {}
async fn measure_temperature() -> u8 {42}
/// Waits for the temperature to change, then returns the new value
async fn wait_temperature_alarm() -> u8 {42}

use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp;
use std::io::Result;
use tokio::select;
use std::pin::pin;

enum Command {
    SetPower(u8),
    QueryTemperature,
}
enum Response {
    Temperature(u8)
}

async fn read_command(reader: &mut tcp::ReadHalf<'_>) -> std::io::Result<Command> {
    todo!() // omitted for brevity
}
async fn write_response(writer: &mut tcp::WriteHalf<'_>, response: Response) -> std::io::Result<()> {
    todo!() // omitted for brevity
}


async fn run_server(stream: &mut TcpStream) -> Result<()> {
    let (mut reader, mut writer) = stream.split();
    
    let mut alarm = pin!(wait_temperature_alarm());
    
    loop {
        select!{
            cmd = read_command(&mut reader) => {
                match cmd? {
                    Command::SetPower(power) => {
                        set_heater_power(power).await;
                    }
                    Command::QueryTemperature => { // Query temperature
                        let temperature = measure_temperature().await;
                        write_response(&mut writer, Response::Temperature(temperature)).await?;
                    }
                }                 
            },
            new_temperature = &mut alarm => {
                write_response(&mut writer, Response::Temperature(new_temperature)).await?;
                alarm.set(wait_temperature_alarm());
            }
        }
    }
}
```

The `alarm` future is now never canceled. Instead, the future is polled to completion. However, the program, as written,
is still not great. Even though the `wait_temperature_alarm` future isn't ever canceled, it is still not scheduled
while `write_response` executes.

### Async code and resource ownership

Now, let's imagine that our hypothetical machine has other features,
and some of these features interfere with the precise temperature measurement. For this reason, we modify
`wait_temperature_alarm` to acquire a lock while performing measurements:

```rust
use tokio::time::{Duration, sleep};

fn enable_measuring_current() {}
fn sample_ad_converter() -> u8 {42}
static PRECISE_MEASUREMENT_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(()); 

async fn wait_temperature_alarm() -> u8 {
    loop {
        sleep(Duration::from_millis(1000)).await;
        {
            let _guard = PRECISE_MEASUREMENT_MUTEX.lock().await;
            enable_measuring_current();
            sleep(Duration::from_millis(1)).await;
            let temperature = sample_ad_converter();
            if temperature > 100 {
                return temperature;
            }
        }
    }
}
```

Imagine that the `measure_temperature` method also acquires `PRECISE_MEASUREMENT_MUTEX`. This will now 
potentially deadlock the system. Futures that exist but are not being actively polled are hard to reason 
about. In many other languages, the `wait_temperature_alarm` method above would be safe against deadlock
(unless `sample_ad_converter` also grabs the lock, but if it's a method of some hardware abstraction layer,
it could be quite easily determined not to).

To be clear, the deadlock can happen because if the future created by `wait_temperature_alarm` stops being
polled, it may have executed `PRECISE_MEASUREMENT_MUTEX.lock().await`, but not
completed `sleep(Duration::from_millis(1)).await;`. Nominally, the sleep returns within 1 ms, plus minus
some jitter. But if the future isn't polled, even the sleep will never complete.

# What we've learned

This post has illustrated three related, but different problems:

1. The dangers of async cancellation (framing error when `read_command` is canceled)
2. The danger of not processing input (deadlock when client + server both write without reading)
3. The danger of non-polled futures owning resources (PRECISE_MEASUREMENT_MUTEX deadlock)

# How should this be solved?

The example presented in this document may be slightly contrived. But every issue illustrated
can happen in real rust code, and worse, requires non-trivial non-local analysis to avoid. The `read_command`
shown isn't buggy in itself, it's just not cancellation safe. Determining if a piece of async code may
be canceled is not possible with only local information. 

The first and third points above can be viewed as being caused directly by a failure to poll futures
to completion.

I'd like to propose the following rule: Futures should always be polled continuously, to completion, except
in exceptional circumstances, such as when canceled intentionally (e.g timeouts). 

Some async API:s require canceling futures to implement timeout functionality. The amount of canceling that is 
needed can be affected by API design. For example, tokio `CancellationToken` can be used to be able to cancel an 
operation without canceling a future.

See [part 2](EXAMPLE.md) for an implementation using a single task without cancellation.

There's an interesting parallel here to aborting threads. The programming community has long since
come to the conclusion that the ability to abort threads "from the outside" causes more harm than benefit.

Rust does not support terminating threads from another thread. Neither does python.
For C#, the ability has been deprecated for a long time, see: 
<https://learn.microsoft.com/en-us/dotnet/core/compatibility/core-libraries/5.0/thread-abort-obsolete>
Java does not support it: <https://docs.oracle.com/javase/tutorial/essential/concurrency/interrupt.html>


## Using separate tasks
The program could be split into three tasks: A reading task, a writing task and a monitoring task.

Both the reading task and the monitoring task need to be able to communicate with the writing task.
This would be most conveniently achieved using something like a tokio mpsc channel.

The downside of this approach is some slight performance overhead for the atomic operations
needed by the channels, and the slight extra complexity from the additional tasks. Such
an approach also may not always eliminate the problem. For example, in a more complex
application, the monitoring task may have adjustable parameters, meaning it must again
be able to receive events from more than one source.


# In the real world

The example presented in this text is simplified. However, the problems illustrated can happen in more
realistic code bases too. Whenever a single task is expected to react to multiple different stimuli
and the code is composed of async methods calling other async methods (or other non-cancel safe futures are
involved), these issues can arise.

# Other viewpoints

## Cancellation isn't that bad
It could be argued that cancellation isn't to be avoided. The programmer just has to ensure that
methods are cancel safe. However, this is often quite difficult in practice. The developer has to
consider the effect of stopping execution at every `.await` point in a cancel safe method. Also,
some methods can't easily be made cancellation safe (e.g, `tokio::sync::mpsc::Sender::send`). 

## Not polling futures isn't that bad
It could be argued that it's okay to have futures that are not being polled. However, this brings
a similar amount of cognitive overhead. It means that even sleeps and timeouts cannot be taken for granted.
The programmer has to consider every `.await` point to last an unbounded amount of time.

It can be argued that this is the case even if futures are constantly polled. After all, there are no
hard performance guarantees in most rust environments. However, without keeping track of which futures are polled,
it can be hard to reason about if a particular program will complete or not. This is especially true if futures
not being polled hold locks. But the same goes for other types of synchronization primitives. For example, a
future blocking on an mpsc channel send (because the channel is full) may cause starvation in other parts of the 
system, and will never complete if the future isn't being polled.
 


# Using the aselect library

See [part 2](EXAMPLE.md) for an implementation of the above example code using aselect.

