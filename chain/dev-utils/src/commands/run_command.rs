use super::init_command::InitCommand;
use failure::{format_err, Error};
use std::fs;
use std::process::Command;
use std::{thread, time};
#[derive(Debug)]
pub struct RunCommand {
    chain_id: String,
    app_hash: String,
}

impl RunCommand {
    pub fn new() -> Self {
        RunCommand {
            chain_id: "".to_string(),
            app_hash: "".to_string(),
        }
    }

    fn read_tendermint_genesis(&mut self) -> Result<(), Error> {
        // check whether file exists
        fs::read_to_string(&InitCommand::get_tendermint_filename())
            .and_then(|contents| {
                println!("current tendermint genesis={}", contents);
                let json: serde_json::Value = serde_json::from_str(&contents).unwrap();
                self.chain_id = json["chain_id"].as_str().unwrap().to_string();
                self.app_hash = json["app_hash"].as_str().unwrap().to_string();
                println!("chain_id={}", self.chain_id);
                println!("app_hash={}", self.app_hash);
                Ok(())
            })
            .map_err(|_e| format_err!("read tendermint genesis error"))
    }

    pub fn run_program(&self, command: &str, arg: Vec<&str>) -> Result<(), Error> {
        Command::new(command)
            .args(arg.as_slice())
            .spawn()
            .map(|_e| {
                println!("{} launched!", command);
            })
            .map_err(|_e| {
                println!("{} error!", command);
                format_err!("{} launch error", command)
            })
            .and_then(|_e| {
                println!("{} run ok", command);
                Ok(())
            })
    }

    pub fn wait(&self, task: &str, milliseconds: u64) -> Result<(), Error> {
        println!("{}", task);
        thread::sleep(time::Duration::from_millis(milliseconds));
        Ok(())
    }

    pub fn execute(&mut self) -> Result<(), Error> {
        println!("run program");
        self.read_tendermint_genesis()
            .and_then(|_| {
                self.run_program(
                    "killall",
                    vec!["tx-validation-app", "tendermint", "chain-abci"],
                )
            })
            .and_then(|_| self.wait("wait for process cleanup", 1000))
            .and_then(|_| self.run_program("./tx-validation-app", vec!["tcp://0.0.0.0:25933"]))
            .and_then(|_| self.wait("wait for booting enclave", 1000))
            .and_then(|_| {
                println!("chain_id={} app_hash={}", self.chain_id, self.app_hash);
                let args = vec![
                    "--host",
                    "0.0.0.0",
                    "--port",
                    "26658",
                    "--chain_id",
                    self.chain_id.as_str(),
                    "--genesis_app_hash",
                    self.app_hash.as_str(),
                    "--enclave_server",
                    "tcp://127.0.0.1:25933",
                ];
                self.run_program("./chain-abci", args)
            })
            .and_then(|_| self.wait("wait for booting abci", 2000))
            .and_then(|_| self.run_program("./tendermint", vec!["node"]))
    }
}
