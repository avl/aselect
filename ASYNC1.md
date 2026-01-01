# Async Design Patterns with aselect

This document takes a look at a few design patterns for async rust programs, and how they can be expressed
using the `aselect` crate. 

## Scenario 1

Let's say you're writing a server for an embedded system. The system is controlled through a custom
protocol built on top of TCP. The system has a set of actuators and sensors. The sensors produce
values that are transmitted to clients over TCP.

We'll choose a "one task per client" approach. Let's start by writing a simple server to control a heater.
Note, this example is simplified. 

```rust

async fn set_heater_power(power: u8) {}

use tokio::net::TcpStream;
use tokio::io::AsyncReadExt;
use std::io::Result;

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

The above example works reliably. Clients write a 1-byte command 1 to adjust power, followed by a parameter with 
the desired power. While `set_heater_power` is executing, no further commands are processed. However,
this seems reasonable.

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

Upon receiving a 2 from the client, the server responds with the current temperature.

Now, let's add an alarm feature. Whenever the temperature exceeds 100, the client should be notified immediately,
without having to perform a request. To achieve this, we need a primitive that allows us to monitor two different
futures for completion.

Tokio provides such a primitive, `tokio::select`:

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

The above program is likely to work well in practice, but it has a subtle bug: If `wait_temperature_alarm`
completes frequently, it may end up saturating the TcpStream send buffer, effectively blocking on `writer.write_u8`.
If the client has somehow managed to fill up its send-queue too, the system will deadlock. This may be quite unlikely
to happen for this simple example, but as a system grows more complex, this type of issue will be more likely.

A similar potential misfeature is that while `set_heater_power` is executing, the alarm feature is not active.

Now, let's leave these concerns for a while and consider cleaning up the program a little. Mixing protocol parsing
and logic like this can make the program harder to reason about. So let's abstract the protocol:



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

However, the program now contains a pretty sever bug. If the temperature changes while a SetPower command is being
read, the future returned by `read_command` will be canceled. But it may already have consumed the first byte of
the 2-byte on-wire packet. In the next iteration of the loop, the 'power' parameter byte will now be interpreted
as a new command. This type of protocol error is called a framing error.

The cause of this framing error in this case is that `read_command` is not "cancel safe". For an excellent
article on cancellation safety, see: <https://rfd.shared.oxide.computer/rfd/400>.

Let's continue this slightly contrived journey, and look at the method `wait_temperature_change`.
Imagine that its innards perform hardware operations like this:

```rust
use tokio::time::Duration;
use tokio::time::sleep;
fn enable_measuring_current() {}
fn sample_ad_converter() -> u8 {42}
async fn wait_temperature_change() -> u8 {
    loop {
        enable_measuring_current();
        sleep(Duration::from_millis(1)).await;
        let temperature = sample_ad_converter();
        if temperature > 100 {
            return temperature;
        }
        sleep(Duration::from_millis(1000)).await;
    }
}
```

Imagine this is measuring some very sensitive chemical process, or whatever, sending current through at PT100 
resistive temperature probe. A precise current is transmitted, and the voltage drop across the actual temperature sensing 
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

Now, let's imagine that our hypothetical machine has other features,
and some of these features interfere with the precise temperature measurement. For this reason, we modify
`wait_temperature_change` to acquire a lock while performing measurements:

```rust
use tokio::time::{Duration, sleep};

fn enable_measuring_current() {}
fn sample_ad_converter() -> u8 {42}
static PRECISE_MEASUREMENT_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(()); 

async fn wait_temperature_change() -> u8 {
    loop {
        enable_measuring_current();
        {
            let _guard = PRECISE_MEASUREMENT_MUTEX.lock().await;
            sleep(Duration::from_millis(1)).await;
            let temperature = sample_ad_converter();
            if temperature > 100 {
                return temperature;
            }
        }
        sleep(Duration::from_millis(1000)).await;
    }
}
```

Imagine that the `measure_temperature` method also acquires `PRECISE_MEASUREMENT_MUTEX`. This will now 
potentially deadlock the system. Futures that exist but are not being actively polled are hard to reason 
about. In many other languages, the `wait_temperature_change` method above would be safe against deadlock
(unless `sample_ad_converter` also grabs the lock, but if it's a method of some hardware abstraction layer,
it could be quite easily determined not to).

To be clear, the deadlock can happen because if the future created by `wait_temperature_change` stop being
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

The first and third points above can be viewed as being caused direction by a failure to poll futures
to completion. 

I'd like to propose the following rule: Futures should always be polled continuously, to completion.

There's an interesting parallel here to aborting threads. The programming community has long since
come to the conclusion that the ability to abort threads "from the outside" causes more harm than benefit.

Rust does not support terminating threads from another thread. Neither does python.
For C#, the ability has been deprecated for a long time, see: 
<https://learn.microsoft.com/en-us/dotnet/core/compatibility/core-libraries/5.0/thread-abort-obsolete>
Java does not support it: <https://docs.oracle.com/javase/tutorial/essential/concurrency/interrupt.html>

# Using the aselect library

See [aselect](ASYNC2.md) for an implementation of the above example code using aselect.




















