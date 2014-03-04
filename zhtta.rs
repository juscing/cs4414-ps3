//
// zhtta.rs
//
// Starting code for PS3
// Running on Rust 0.9
//
// Note that this code has serious security risks!  You should not run it 
// on any system with access to sensitive files.
// 
// University of Virginia - cs4414 Spring 2014
// Weilin Xu and David Evans
// Version 0.5

// To see debug! outputs set the RUST_LOG environment variable, e.g.: export RUST_LOG="zhtta=debug"

// Justin Ingram (jci5kb)
// Brian Whitlow (btw2cv)
#[feature(globs)];
extern mod extra;


use std::io::*;
use std::io::net::ip::{SocketAddr};
use std::{os, str, libc, from_str};
use std::path::Path;
use std::hashmap::HashMap;
use std::io::buffered::BufferedReader;

use extra::getopts;
use extra::arc::MutexArc;
use extra::arc::RWArc;
use extra::lru_cache::LruCache;
use extra::flate::*;

mod gash;


static SERVER_NAME : &'static str = "Zhtta Version 0.5";

static IP : &'static str = "127.0.0.1";
static PORT : uint = 4414;
static WWW_DIR : &'static str = "./www";

static HTTP_OK : &'static str = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\n\r\n";
static HTTP_OK_DEFLATE : &'static str = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Encoding: deflate\r\n\r\n";

static HTTP_BAD : &'static str = "HTTP/1.1 404 Not Found\r\n\r\n";

static HTTP_OK_BIN : &'static str = "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream;\r\n\r\n";
static HTTP_OK_BIN_DEFLATE : &'static str = "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream;\r\nContent-Encoding: deflate\r\n\r\n";

static COUNTER_STYLE : &'static str = "<doctype !html><html><head><title>Hello, Rust!</title>
             <style>body { background-color: #884414; color: #FFEEAA}
                    h1 { font-size:2cm; text-align: center; color: black; text-shadow: 0 0 4mm red }
                    h2 { font-size:2cm; text-align: center; color: black; text-shadow: 0 0 4mm green }
             </style></head>
             <body>";

static CORES_AVAILABLE : uint = 1;

struct HTTP_Request {
    // Use peer_name as the key to access TcpStream in hashmap. 

    // (Due to a bug in extra::arc in Rust 0.9, it is very inconvenient to use TcpStream without the "Freeze" bound.
    //  See issue: https://github.com/mozilla/rust/issues/12139)
    peer_name: ~str,
    path: ~Path,
}

struct WebServer {
    ip: ~str,
    port: uint,
    www_dir_path: ~Path,
    
    request_queue_arc: MutexArc<~[HTTP_Request]>,
    stream_map_arc: MutexArc<HashMap<~str, Option<std::io::net::tcp::TcpStream>>>,
    
    notify_port: Port<()>,
    shared_notify_chan: SharedChan<()>,
    
    visitor_count: RWArc<uint>,
    dequeue_task_count: RWArc<uint>,

    WahooFirst_count: RWArc<uint>,
    
    cache: RWArc<LruCache<~str, ~[u8]>>,
}

impl WebServer {
    fn new(ip: &str, port: uint, www_dir: &str) -> WebServer {
        let (notify_port, shared_notify_chan) = SharedChan::new();
        let www_dir_path = ~Path::new(www_dir);
        os::change_dir(www_dir_path.clone());

        WebServer {
            ip: ip.to_owned(),
            port: port,
            www_dir_path: www_dir_path,
                        
            request_queue_arc: MutexArc::new(~[]),
            stream_map_arc: MutexArc::new(HashMap::new()),
            
            notify_port: notify_port,
            shared_notify_chan: shared_notify_chan,      
            
            visitor_count: RWArc::new(0),
            WahooFirst_count: RWArc::new(0),

            dequeue_task_count: RWArc::new(0),
            
            cache: RWArc::new(LruCache::new(10)),
        }
    }
    
    fn run(&mut self) {
        self.listen();
        self.dequeue_static_file_request();
    }
    
    fn listen(&mut self) {
        let addr = from_str::<SocketAddr>(format!("{:s}:{:u}", self.ip, self.port)).expect("Address error.");
        let www_dir_path_str = self.www_dir_path.as_str().expect("invalid www path?").to_owned();
        
        let request_queue_arc = self.request_queue_arc.clone();
        let shared_notify_chan = self.shared_notify_chan.clone();
        let stream_map_arc = self.stream_map_arc.clone();
        
        let counter = self.visitor_count.clone();

        let wahooTemp1 = self.WahooFirst_count.clone();
        
        spawn(proc() {
            let mut acceptor = net::tcp::TcpListener::bind(addr).listen();
            println!("{:s} listening on {:s} (serving from: {:s}).", 
                     SERVER_NAME, addr.to_str(), www_dir_path_str);
            
            for stream in acceptor.incoming() {
                let (queue_port, queue_chan) = Chan::new();
                queue_chan.send(request_queue_arc.clone());
                
                let notify_chan = shared_notify_chan.clone();
                let stream_map_arc = stream_map_arc.clone();
                let countertwo = counter.clone();
                let wahooTemp = wahooTemp1.clone();
                
                // Spawn a task to handle the connection.
                spawn(proc() {
                    countertwo.write(|state| {
			             *state += 1;
                    });
                    let request_queue_arc = queue_port.recv();
                  
                    let mut stream = stream;
                    
                    let peer_name = WebServer::get_peer_name(&mut stream);
                    
                    let mut buf = [0, ..500];
                    stream.read(buf);
                    let request_str = str::from_utf8(buf);
                    debug!("Request:\n{:s}", request_str);
                    
                    let req_group : ~[&str]= request_str.splitn(' ', 3).collect();
                    if req_group.len() > 2 {
                        let path_str = "." + req_group[1].to_owned();
                        
                        let mut path_obj = ~os::getcwd();
                        path_obj.push(path_str.clone());
                        
                        let ext_str = match path_obj.extension_str() {
                            Some(e) => e,
                            None => "",
                        };
                        
                        debug!("Requested path: [{:s}]", path_obj.as_str().expect("error"));
                        debug!("Requested path: [{:s}]", path_str);
                             
                        if path_str == ~"./" {
                            debug!("===== Counter Page request =====");
                            let mut countcopy = 0;
                            countertwo.read(|state| {
				                countcopy = state.clone();
                            });
                            WebServer::respond_with_counter_page(stream, countcopy);
                            debug!("=====Terminated connection from [{:s}].=====", peer_name);
                        } else if !path_obj.exists() || path_obj.is_dir() {
                            debug!("===== Error page request =====");
                            WebServer::respond_with_error_page(stream, path_obj);
                            debug!("=====Terminated connection from [{:s}].=====", peer_name);
                        } else if ext_str == "shtml" { // Dynamic web pages.
                            debug!("===== Dynamic Page request =====");
                            WebServer::respond_with_dynamic_page(stream, path_obj);
                            debug!("=====Terminated connection from [{:s}].=====", peer_name);
                        } else { 
                            debug!("===== Static Page request =====");
                            
                            WebServer::enqueue_static_file_request(stream, path_obj, stream_map_arc, request_queue_arc, notify_chan, wahooTemp);
                        }
                    }
                });
            }
        });
    }

    fn respond_with_error_page(stream: Option<std::io::net::tcp::TcpStream>, path: &Path) {
        let mut stream = stream;
        let msg: ~str = format!("Cannot open: {:s}", path.as_str().expect("invalid path").to_owned());

        stream.write(HTTP_BAD.as_bytes());
        stream.write(msg.as_bytes());
    }

    fn respond_with_counter_page(stream: Option<std::io::net::tcp::TcpStream>, count: uint) {
        let mut stream = stream;
        let response: ~str = 
            format!("{:s}<h1>Greetings, Krusty!</h1>
                     <h2>Visitor count: {:u}</h2></body></html>\r\n", 
                    COUNTER_STYLE, 
                    count );
        debug!("Responding to counter request");
        stream.write(HTTP_OK_DEFLATE.as_bytes());
        stream.write(deflate_bytes(response.as_bytes()));
    }
    
    // TODO: Streaming file.
    // TODO: Application-layer file caching.
    fn respond_with_static_file(stream: Option<std::io::net::tcp::TcpStream>, path: &Path, cache: RWArc<LruCache<~str, ~[u8]>>) {
        let mut stream = stream;
        
        
        let mut storevec: ~[u8] = ~[];
        //Justin's better file streaming...
        
        let path_str = match path.as_str() {
	    Some(stringy) => {stringy.to_owned()}
	    None => {fail!("Path not representable in UTF-8");}
        };
        
                
        debug!("Waiting for LRU_Cache access for read... ");
        let mut in_cache = true;
        cache.write(|state| {
            debug!("Received LRU_Cache read access!");
            match state.get(&path_str) {
                Some(data) => {
                    debug!("Already in Cache!");
                    if path_str.ends_with(".html") {
                        stream.write(HTTP_OK_DEFLATE.as_bytes());
                    }else{
                        stream.write(HTTP_OK_BIN_DEFLATE.as_bytes());
                    }
                    stream.write(*data);
                    }
                None => {
                    in_cache = false;
                }
            }
        });
        if in_cache == false {
            let file_reader = File::open(path).expect("Invalid file!");
            let mut br = BufferedReader::new(file_reader);
            debug!("Not in the Cache...");
            if path_str.ends_with(".html") {
                stream.write(HTTP_OK.as_bytes());
            }else{
                stream.write(HTTP_OK_BIN.as_bytes());
            }
            while !br.eof() {
		/*
                match file_reader.read_byte() {
                    Some(bytes) => {
                        storevec.push(bytes);
                        stream.write_u8(bytes);
                    }
                    None => {}
                }
                */
                let l;
                {
		    let x = br.fill();
		    l = x.len();
		    stream.write(x);
		    storevec.push_all_move(x.into_owned());
                }
                br.consume(l);
            }
            debug!("Waiting for LRU_Cache access for write...")
            cache.write(|state| {
                debug!("Received LRU_Cache write access!");
                state.put(path_str.clone(), deflate_bytes(storevec));
            });
            
        }
            
            //I know this is weird but its something to do with a double borrow...
                        
        
        
        
    }
    
    fn respond_with_dynamic_page(stream: Option<std::io::net::tcp::TcpStream>, path: &Path) {
        let mut stream = stream;
        let mut file_reader = File::open(path).expect("Invalid file!");
        let file_content  = file_reader.read_to_end();
        let raw_content = str::from_utf8(file_content);

        let split_content: ~[&str]  = raw_content.split_str("<!--").collect();
        if split_content.len() == 1 {
            stream.write(HTTP_OK_DEFLATE.as_bytes());
            stream.write(deflate_bytes(raw_content.as_bytes()));
        } else {
            stream.write(HTTP_OK_DEFLATE.as_bytes());
            let mut resvec: ~[u8] = ~[];
            for i in range (0, split_content.len()) {
                let content: ~[&str]  = split_content[i].split_str("-->").collect();
                if content.len() > 1 {
                    match content[0].slice_to(10) {
                        &"#exec cmd=" => {
                            let cmd: ~[&str] = content[0].split('"').collect();
                            let response = gash::run_cmdline(cmd[1]);

                            //stream.write(response.as_bytes());
                            resvec.push_all_move(response.as_bytes().into_owned());
                        }
                        _ => {
                            debug!("This command doesn't seem to make sense: {:s}", content[0])
                        }
                    }

                } else {
                    //stream.write(split_content[i].as_bytes());
                    resvec.push_all_move(split_content[i].as_bytes().into_owned());
                }
            }
            stream.write(deflate_bytes(resvec));
        }
        
    }
    
    // TODO: Smarter Scheduling.
    fn enqueue_static_file_request(stream: Option<std::io::net::tcp::TcpStream>, path_obj: &Path, stream_map_arc: MutexArc<HashMap<~str, Option<std::io::net::tcp::TcpStream>>>, req_queue_arc: MutexArc<~[HTTP_Request]>, notify_chan: SharedChan<()>, WahooFirst: RWArc<uint>) {
        // Save stream in hashmap for later response.
        let mut stream = stream;
        let peer_name = WebServer::get_peer_name(&mut stream);
        let (stream_port, stream_chan) = Chan::new();
        stream_chan.send(stream);
        unsafe {
            // Use an unsafe method, because TcpStream in Rust 0.9 doesn't have "Freeze" bound.
            stream_map_arc.unsafe_access(|local_stream_map| {
                let stream = stream_port.recv();
                local_stream_map.swap(peer_name.clone(), stream);
            });
        }
        
        // Enqueue the HTTP request.
        let temp = peer_name.slice_to(7);
        //debug!("Testing WahooFirst")
        let mut prioritize = false;
        if ((temp == "128.143") || (temp == "137.54.")) {
            debug!("WahooFirst Priority enabled for request!");
            prioritize = true;
        }
        
        let req = HTTP_Request { peer_name: peer_name.clone(), path: ~path_obj.clone() };
        let (req_port, req_chan) = Chan::new();
        req_chan.send(req);

        debug!("Waiting for queue mutex lock.");

        let mut WahooFirst_count = 0;
        WahooFirst.read(|state| {
            WahooFirst_count = state.clone();
        });
        req_queue_arc.access(|local_req_queue| {
            debug!("Got queue mutex lock.");
            let req: HTTP_Request = req_port.recv();
            if (prioritize == false) {
                let mut index = WahooFirst_count;
                let compare_size = path_obj.stat().size;
                let mut size_queuedfile = compare_size;

                debug!("index: {:u} ", index);
                while (index < local_req_queue.len()-1 && compare_size < size_queuedfile) {
                    size_queuedfile = local_req_queue[index].path.stat().size;
                    index += 1;
                }
                local_req_queue.insert(index,req);
            } else {
                local_req_queue.insert(WahooFirst_count, req);
                WahooFirst_count += 1;
            }
            debug!("A new request enqueued, now the length of queue is {:u}.", local_req_queue.len());
        });

        WahooFirst.write(|state| {
            *state = WahooFirst_count;
        });
        
        notify_chan.send(()); //  incoming notification to responder task.
    
    
    }
    
    // TODO: Smarter Scheduling.
    fn dequeue_static_file_request(&mut self) {
        let req_queue_get = self.request_queue_arc.clone();
        let stream_map_get = self.stream_map_arc.clone();
        
        // Port<> cannot be sent to another task. So we have to make this task as the main task that can access self.notify_port.
        
        let (request_port, request_chan) = Chan::new();
        loop {
            self.notify_port.recv();    // waiting for new request enqueued.
            
            req_queue_get.access( |req_queue| {
                match req_queue.shift_opt() { // FIFO queue.
                    None => { /* do nothing */ }
                    Some(req) => {
                        request_chan.send(req);
                        debug!("A new request dequeued, now the length of queue is {:u}.", req_queue.len());
                    }
                }
            });
            
            let request = request_port.recv();
            
            // Get stream from hashmap.
            // Use unsafe method, because TcpStream in Rust 0.9 doesn't have "Freeze" bound.
            let (stream_port, stream_chan) = Chan::new();
            unsafe {
                stream_map_get.unsafe_access(|local_stream_map| {
                    let stream = local_stream_map.pop(&request.peer_name).expect("no option tcpstream");
                    stream_chan.send(stream);
                });
            }

            //If there is a priority WahooFirst, then it will be the first removed no matter what. 
            
            self.WahooFirst_count.write(|state| {
                if state.clone() > 0 {
                    *state = state.clone()-1;
                }
            });

            
            let cacheclone = self.cache.clone();
            let mut num_threads = -1;
            while (num_threads < 0 || num_threads > 3*CORES_AVAILABLE) {
                self.dequeue_task_count.read(|state| {
                    num_threads = state.clone();
                });
            }
            let dequeue_count_clone = self.dequeue_task_count.clone();
            spawn(proc() {
                dequeue_count_clone.write(|state| {
                    *state += 1;
                });
                let stream = stream_port.recv();
                WebServer::respond_with_static_file(stream, request.path, cacheclone);
                debug!("=====Terminated connection from [{:s}].=====", request.peer_name);
                dequeue_count_clone.write(|state| {
                    *state -= 1;
                });
            });
        }
    }
    
    fn get_peer_name(stream: &mut Option<std::io::net::tcp::TcpStream>) -> ~str {
        match *stream {
            Some(ref mut s) => {
                         match s.peer_name() {
                            Some(pn) => {pn.to_str()},
                            None => (~"")
                         }
                       },
            None => (~"")
        }
    }
}

fn get_args() -> (~str, uint, ~str) {
    fn print_usage(program: &str) {
        println!("Usage: {:s} [options]", program);
        println!("--ip     \tIP address, \"{:s}\" by default.", IP);
        println!("--port   \tport number, \"{:u}\" by default.", PORT);
        println!("--www    \tworking directory, \"{:s}\" by default", WWW_DIR);
        println("-h --help \tUsage");
    }
    
    /* Begin processing program arguments and initiate the parameters. */
    let args = os::args();
    let program = args[0].clone();
    
    let opts = ~[
        getopts::optopt("ip"),
        getopts::optopt("port"),
        getopts::optopt("www"),
        getopts::optflag("h"),
        getopts::optflag("help")
    ];

    let matches = match getopts::getopts(args.tail(), opts) {
        Ok(m) => { m }
        Err(f) => { fail!(f.to_err_msg()) }
    };

    if matches.opt_present("h") || matches.opt_present("help") {
        print_usage(program);
        unsafe { libc::exit(1); }
    }
    
    let ip_str = if matches.opt_present("ip") {
                    matches.opt_str("ip").expect("invalid ip address?").to_owned()
                 } else {
                    IP.to_owned()
                 };
    
    let port:uint = if matches.opt_present("port") {
                        from_str::from_str(matches.opt_str("port").expect("invalid port number?")).expect("not uint?")
                    } else {
                        PORT
                    };
    
    let www_dir_str = if matches.opt_present("www") {
                        matches.opt_str("www").expect("invalid www argument?") 
                      } else { WWW_DIR.to_owned() };
    
    (ip_str, port, www_dir_str)
}

fn main() {
    let (ip_str, port, www_dir_str) = get_args();
    let mut zhtta = WebServer::new(ip_str, port, www_dir_str);
    zhtta.run();
}
