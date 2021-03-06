// Copyright (c) 2013-2014 Sandstorm Development Group, Inc. and contributors
// Licensed under the MIT License:
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

extern crate capnp;
extern crate fdstream;
extern crate rand;

pub mod common;

pub mod carsales_capnp {
  include!(concat!(env!("OUT_DIR"), "/carsales_capnp.rs"));
}
pub mod carsales;

pub mod catrank_capnp {
  include!(concat!(env!("OUT_DIR"), "/catrank_capnp.rs"));
}
pub mod catrank;

pub mod eval_capnp {
  include!(concat!(env!("OUT_DIR"), "/eval_capnp.rs"));
}

pub mod eval;

mod uncompressed {
    pub use capnp::serialize::{read_message, write_message};
}

mod packed {
    pub use capnp::serialize_packed::{read_message, write_message};
}

const SCRATCH_SIZE : usize = 128 * 1024;

#[derive(Clone, Copy)]
pub struct NoScratch;

impl NoScratch {
    fn new_builder(&mut self, _idx : usize) -> capnp::message::Builder<capnp::message::HeapAllocator> {
        capnp::message::Builder::new_default()
    }
}

pub struct UseScratch {
    _owned_space: ::std::vec::Vec<::std::vec::Vec<capnp::Word>>,
    scratch_space: ::std::vec::Vec<::capnp::message::ScratchSpace<'static>>,
}

impl UseScratch {
    pub fn new() -> UseScratch {
        let mut owned = Vec::new();
        let mut scratch = Vec::new();
        for _ in 0..6 {
            let mut words = ::capnp::Word::allocate_zeroed_vec(SCRATCH_SIZE);
            scratch.push(::capnp::message::ScratchSpace::new(
                unsafe {::std::mem::transmute(&mut words[..])}));
            owned.push(words);
        }
        UseScratch {
            _owned_space: owned,
            scratch_space: scratch,
        }
    }

    fn new_builder<'a>(&mut self, idx: usize) -> capnp::message::Builder<capnp::message::ScratchSpaceHeapAllocator<'a, 'a>> {
        assert!(idx < 6);
        capnp::message::Builder::new(::capnp::message::ScratchSpaceHeapAllocator::new(
            unsafe{::std::mem::transmute(&mut self.scratch_space[idx])})) // XXX
    }
}


macro_rules! pass_by_object(
    ( $testcase:ident, $reuse:ident, $iters:expr ) => ({
            let mut rng = common::FastRand::new();
            for _ in 0..$iters {
                let mut message_req = $reuse.new_builder(0);
                let mut message_res = $reuse.new_builder(1);

                let expected = $testcase::setup_request(&mut rng,
                                                        message_req.init_root::<$testcase::RequestBuilder>());

                $testcase::handle_request(message_req.get_root::<$testcase::RequestBuilder>().unwrap().as_reader(),
                                          message_res.init_root::<$testcase::ResponseBuilder>());

                if !$testcase::check_response(
                    message_res.get_root::<$testcase::ResponseBuilder>().unwrap().as_reader(),
                    expected) {
                    panic!("Incorrect response.");
                }
            }
        });
    );


macro_rules! pass_by_bytes(
    ( $testcase:ident, $reuse:ident, $compression:ident, $iters:expr ) => ({
        let mut request_bytes : ::std::vec::Vec<u8> =
            ::std::iter::repeat(0u8).take(SCRATCH_SIZE * 8).collect();
        let mut response_bytes : ::std::vec::Vec<u8> =
            ::std::iter::repeat(0u8).take(SCRATCH_SIZE * 8).collect();
        let mut rng = common::FastRand::new();
        for _ in 0..$iters {
            let mut message_req = $reuse.new_builder(0);
            let mut message_res = $reuse.new_builder(1);

            let expected = {
                let request = message_req.init_root::<$testcase::RequestBuilder>();
                $testcase::setup_request(&mut rng, request)
            };

            {
                let response = message_res.init_root::<$testcase::ResponseBuilder>();

                {
                    let mut writer : &mut[u8] = &mut request_bytes;
                    $compression::write_message(&mut writer, &mut message_req).unwrap()
                }

                let mut request_bytes1 : &[u8] = &request_bytes;
                let message_reader = $compression::read_message(
                    &mut request_bytes1,
                    capnp::message::DEFAULT_READER_OPTIONS).unwrap();

                let request_reader : $testcase::RequestReader = message_reader.get_root().unwrap();
                $testcase::handle_request(request_reader, response);
            }

            {
                let mut writer : &mut [u8] = &mut response_bytes;
                $compression::write_message(&mut writer, &mut message_res).unwrap()
            }

            let mut response_bytes1 : &[u8] = &response_bytes;
            let message_reader = $compression::read_message(
                &mut response_bytes1,
                capnp::message::DEFAULT_READER_OPTIONS).unwrap();

            let response_reader : $testcase::ResponseReader = message_reader.get_root().unwrap();
            if !$testcase::check_response(response_reader, expected) {
                panic!("Incorrect response.");
            }
        }
    });
    );

macro_rules! server(
    ( $testcase:ident, $reuse:ident, $compression:ident, $iters:expr, $input:expr, $output:expr) => ({
            let mut out_buffered = ::std::io::BufWriter::new(&mut $output);
            let mut in_buffered = ::std::io::BufReader::new(&mut $input);
            for _ in 0..$iters {
                let mut message_res = $reuse.new_builder(0);

                {
                    let response = message_res.init_root::<$testcase::ResponseBuilder>();
                    let message_reader = $compression::read_message(
                        &mut in_buffered,
                        capnp::message::DEFAULT_READER_OPTIONS).unwrap();
                    let request_reader : $testcase::RequestReader = message_reader.get_root().unwrap();
                    $testcase::handle_request(request_reader, response);
                }

                $compression::write_message(&mut out_buffered, &mut message_res).unwrap();
                out_buffered.flush().unwrap();
            }
        });
    );

macro_rules! sync_client(
    ( $testcase:ident, $reuse:ident, $compression:ident, $iters:expr) => ({
            let mut out_stream = ::fdstream::FdStream::new(1);
            let mut in_stream = ::fdstream::FdStream::new(0);
            let mut in_buffered = ::std::io::BufReader::new(&mut in_stream);
            let mut out_buffered = ::std::io::BufWriter::new(&mut out_stream);
            let mut rng = common::FastRand::new();
            for _ in 0..$iters {
                let mut message_req = $reuse.new_builder(0);

                let expected = {
                    let request = message_req.init_root::<$testcase::RequestBuilder>();
                    $testcase::setup_request(&mut rng, request)
                };
                $compression::write_message(&mut out_buffered, &mut message_req).unwrap();
                out_buffered.flush().unwrap();

                let message_reader = $compression::read_message(
                    &mut in_buffered,
                    capnp::message::DEFAULT_READER_OPTIONS).unwrap();
                let response_reader : $testcase::ResponseReader = message_reader.get_root().unwrap();
                assert!($testcase::check_response(response_reader, expected));

            }
        });
    );


macro_rules! pass_by_pipe(
    ( $testcase:ident, $reuse:ident, $compression:ident, $iters:expr) => ({
        use std::process;

        let mut args : Vec<String> = ::std::env::args().collect();
        args[2] = "client".to_string();

        let mut command = process::Command::new(&args[0]);
        command.args(&args[1..args.len()]);
        command.stdin(process::Stdio::piped());
        command.stdout(process::Stdio::piped());
        command.stderr(process::Stdio::null());
        match command.spawn() {
            Ok(ref mut p) => {
                let mut child_std_out = p.stdout.take().unwrap();
                let mut child_std_in = p.stdin.take().unwrap();

                server!($testcase, $reuse, $compression, $iters, child_std_out, child_std_in);
                println!("{}", p.wait().unwrap());
            }
            Err(e) => {
                println!("could not start process: {}", e);
            }
        }
    });
    );

macro_rules! do_testcase(
    ( $testcase:ident, $mode:expr, $reuse:ident, $compression:ident, $iters:expr ) => ({
            match &*$mode {
                "object" => pass_by_object!($testcase, $reuse, $iters),
                "bytes" => pass_by_bytes!($testcase, $reuse, $compression, $iters),
                "client" => sync_client!($testcase, $reuse, $compression, $iters),
                "server" => {
                    let mut input = ::fdstream::FdStream::new(0);
                    let mut output = ::fdstream::FdStream::new(1);
                    server!($testcase, $reuse, $compression, $iters, input, output)
                }
                "pipe" => pass_by_pipe!($testcase, $reuse, $compression, $iters),
                s => panic!("unrecognized mode: {}", s)
            }
        });
    );

macro_rules! do_testcase1(
    ( $testcase:expr, $mode:expr, $reuse:ident, $compression:ident, $iters:expr) => ({
            match &*$testcase {
                "carsales" => do_testcase!(carsales, $mode, $reuse, $compression, $iters),
                "catrank" => do_testcase!(catrank, $mode, $reuse, $compression, $iters),
                "eval" => do_testcase!(eval, $mode, $reuse, $compression, $iters),
                s => panic!("unrecognized test case: {}", s)
            }
        });
    );

macro_rules! do_testcase2(
    ( $testcase:expr, $mode:expr, $reuse:expr, $compression:ident, $iters:expr) => ({
            match &*$reuse {
                "no-reuse" => {
                    let mut scratch = NoScratch;
                    do_testcase1!($testcase, $mode, scratch, $compression, $iters)
                }
                "reuse" => {
                    let mut scratch = UseScratch::new();
                    do_testcase1!($testcase, $mode, scratch, $compression, $iters)
                }
                s => panic!("unrecognized reuse option: {}", s)
            }
        });
    );

pub fn main() {
    use ::std::io::{Read, Write};
    let args : Vec<String> = ::std::env::args().collect();

    assert!(args.len() == 6,
            "USAGE: {} CASE MODE REUSE COMPRESSION ITERATION_COUNT",
            args[0]);

    let iters = match args[5].parse::<u64>() {
        Ok(n) => n,
        Err(_) => {
            panic!("Could not parse a u64 from: {}", args[5]);
        }
    };

    match &*args[4] {
        "none" => do_testcase2!(args[1], args[2],  args[3], uncompressed, iters),
        "packed" => do_testcase2!(args[1], args[2], args[3], packed, iters),
        s => panic!("unrecognized compression: {}", s)
    }
}
