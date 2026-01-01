use std::collections::VecDeque;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp;
use std::io::Result;
use std::time::Duration;
use aselect::{aselect, Output};

async fn set_heater_power(power: u8) {
    println!("Heater power: {}", power);
}
async fn measure_temperature() -> u8 {42}

/// Waits for the temperature to change, then returns the new value
async fn wait_temperature_alarm() -> u8 {
    tokio::time::sleep(Duration::from_secs(10)).await;
    100
}

enum Command {
    SetPower(u8),
    QueryTemperature,
}
enum Response {
    Temperature(u8)
}

async fn read_command(reader: &mut tcp::ReadHalf<'_>) -> std::io::Result<Command> {
    let cmd = reader.read_u8().await?;
    Ok(match cmd {
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

async fn run_client(stream: TcpStream) {
    let (mut reader,mut writer) = stream.into_split();

    tokio::spawn(async move {
        println!("Sending command");
        loop {
            writer.write_u8(1).await.unwrap(); // Set power
            writer.write_u8(43).await.unwrap(); // to 43
            writer.write_u8(2).await.unwrap(); // Read
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });

    loop {
        let response = reader.read_u8().await.unwrap();
        assert_eq!(response, 2);
        let temperature = reader.read_u8().await.unwrap();
        println!("Temperature is now {}", temperature);
    }
}

#[tokio::main]
async fn main() {
    if std::env::var("CLIENT").is_ok() {
        println!("Running as client");
        let stream = TcpStream::connect("127.0.0.1:8080").await.unwrap();
        run_client(stream).await;
    } else {
        println!("Running as server");
        let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();
        while let Ok((mut stream, _)) = listener.accept().await {
            println!("Accepted client");
            tokio::spawn(async move {
                let res  = run_server(&mut stream).await;
                println!("Server exited: {:?}", res);
                res.unwrap();
            }).await.unwrap();
        }
    }
}
