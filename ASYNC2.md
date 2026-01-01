# Using aselect

Before reading this, see  [part 1](ASYNC1.md) for a description of the problem being solved.


The following rust code, using the aselect-library, avoids all problems of the previous code.

```rust

async fn run_server(stream: &mut TcpStream) -> Result<()> {
    let (reader, writer) = stream.split();

    let new_power : Option<u8> = None;
    let perform_measurement = false;
    let queued_responses : VecDeque<Response> = VecDeque::new();

    aselect!(
        {
            mutable(new_power, queued_responses, perform_measurement);
            borrowed(reader, writer);
        },
        read(
            {},
            async |_temp, reader| {
                read_command(reader).await
            },
            |cmd| {
                match cmd {
                    Ok(Command::SetPower(power)) => {
                        *new_power = Some(power);
                    }
                    Ok(Command::QueryTemperature) => { // Query temperature
                        *perform_measurement = true;
                    }
                    Err(err) => {
                        return Some(Output::Value(Err(err)));
                    }
                }
                None
            }
        ),
        write(
            {
                queued_responses.pop_front()?
            },
            async |response, writer| {
                write_response(writer, response).await
            },
            |result|{
                if let Err(err) = result {
                    return Some(Output::Value(Err(err)));
                }
                 None
            }

        ),
        set_power(
            {
                new_power.take()?
            },
            async |power|{
                set_heater_power(power)
            },
            |_result|
            {
                None
            }
        ),
        measure(
            {
                if !*perform_measurement {
                    return None;
                }
                *perform_measurement = false;
            },
            async |_temp|{
                measure_temperature().await
            },
            |temperature|
            {
                queued_responses.push_back(Response::Temperature(temperature));
                None
            }
        ),
        alarm(
            {
            },
            async |_temp| {
                wait_temperature_alarm().await
            },
            |temperature|{
                queued_responses.push_back(Response::Temperature(temperature));
                None
            }
        )
    ).await

}
```

Let's go through it part-by-part.

First, we define some state:

```rust
    let new_power : Option<u8> = None;
    let perform_measurement = false;
    let queued_responses : VecDeque<Response> = VecDeque::new();
```

 * `new_power` is an option that is set to whatever new value the heater power has been commanded to, or None if
   no command is active.
 * `perform_measurement` is set to true whenever a measurement has been desired.
 * `queued_responses` contains responses that have been created, but not yet transmitted to the client.

Then we invoke the `aselect` macro:


```rust
    aselect!(
        {
            mutable(new_power, queued_responses, perform_measurement);
            borrowed(reader, writer);
        },
    ...
    ).await
```

Here we define `new_power`, `queued_responses` and `perform_measurement` as mutable captures. These
variables can be accessed directly from within the setup and handler blocks (which we'll learn more about below).

We define `reader` and `writer` as "borrowed" captures. This means that they can be borrowed by async blocks. This 
means async blocks that use `reader` and `writer` can capture a reference to these variables in their future.
If two async blocks try to capture the same variable, only the first one will actually run. The reason for this is
that mutable references to the same captured variable cannot be held by two different futures, because of rust's
borrow rules.

Now, let's look at the first select arm (commented):


```rust 
        read(
            {}, //Setup
            async |_temp, reader| { //Async block
                read_command(reader).await
            },
            |cmd| { // Handler
                match cmd {
                    Ok(Command::SetPower(power)) => {
                        *new_power = Some(power);
                    }
                    Ok(Command::QueryTemperature) => { // Query temperature
                        *perform_measurement = true;
                    }
                    Err(err) => {
                        return Some(Output::Value(Err(err)));
                    }
                }
                None
            }
        ),
```
The arm is named `read`, and has three blocks:
 * 
 * setup
 * async
 * handler

The setup is empty. The async block simply calls the async method `read_command`, with the borrowed capture `reader`.
Every "borrowed" capture must be specified in the argument list to an async block. The same capture can be used in
multiple blocks, but only the first enabled such future will actually run.

Finally, the result produced by the async code is given to the handler, which acts on the parsed command.

If the received command i `SetPower`, we set the mutable capture `new_power` to the new desired power value.
If the received command i `QueryTemperature`, we set the mutable capture `perform_measurement` to true.

This block allows us to receive commands, and update our shared state as a result of those commands.
Note that the future created by the `read_command` async method will never be canceled. It thus does not need
to be cancelation safe.

The next block is:

TODO! Will be continued!



