use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;

use crate::http::{parse_request, parse_response_head, HttpRequest, ParseError, ResponseBodyMode, MAX_HEADER_BYTES};

const BUFFER_SIZE: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024 * 1024;
const BACKEND_COOLDOWN: Duration = Duration::from_secs(5);
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_RETRIES: usize = 2;

#[derive(Debug, Clone)]
pub struct ProxyConfig { pub backends: Vec<SocketAddr>, pub connect_timeout: Duration, pub request_timeout: Duration, pub max_retries: usize }
impl Default for ProxyConfig {
    fn default() -> Self { Self { backends: vec![SocketAddr::from(([127,0,0,1],9000))], connect_timeout: DEFAULT_CONNECT_TIMEOUT, request_timeout: DEFAULT_REQUEST_TIMEOUT, max_retries: DEFAULT_RETRIES } }
}
impl ProxyConfig {
    pub fn from_env() -> Result<Self, ProxyError> {
        let backends = match std::env::var("ATLAS_BACKENDS") { Ok(v) => parse_backends(&v)?, Err(_) => Self::default().backends };
        if backends.is_empty() { return Err(ProxyError::NoBackends); }
        let connect_timeout = env_duration("ATLAS_CONNECT_TIMEOUT_MS", DEFAULT_CONNECT_TIMEOUT)?;
        let request_timeout = env_duration("ATLAS_REQUEST_TIMEOUT_MS", DEFAULT_REQUEST_TIMEOUT)?;
        let max_retries = std::env::var("ATLAS_MAX_RETRIES").ok().map(|v| v.parse::<usize>()).transpose().map_err(|_| ProxyError::InvalidConfiguration("ATLAS_MAX_RETRIES"))?.unwrap_or(DEFAULT_RETRIES);
        Ok(Self { backends, connect_timeout, request_timeout, max_retries })
    }
}

#[derive(Debug, Default)]
pub struct ProxyMetrics { requests_total: AtomicU64, requests_succeeded: AtomicU64, requests_failed: AtomicU64, upstream_failures: AtomicU64, upstream_timeouts: AtomicU64, bytes_to_client: AtomicU64 }
impl ProxyMetrics {
    fn request_started(&self) { self.requests_total.fetch_add(1, Ordering::Relaxed); }
    fn request_succeeded(&self, bytes: usize) { self.requests_succeeded.fetch_add(1, Ordering::Relaxed); self.bytes_to_client.fetch_add(bytes as u64, Ordering::Relaxed); }
    fn request_failed(&self) { self.requests_failed.fetch_add(1, Ordering::Relaxed); }
    fn upstream_failure(&self) { self.upstream_failures.fetch_add(1, Ordering::Relaxed); }
    fn upstream_timeout(&self) { self.upstream_timeouts.fetch_add(1, Ordering::Relaxed); }
    pub fn render_prometheus(&self) -> String { format!("# TYPE atlas_requests_total counter\natlas_requests_total {}\n# TYPE atlas_requests_succeeded counter\natlas_requests_succeeded {}\n# TYPE atlas_requests_failed counter\natlas_requests_failed {}\n# TYPE atlas_upstream_failures counter\natlas_upstream_failures {}\n# TYPE atlas_upstream_timeouts counter\natlas_upstream_timeouts {}\n# TYPE atlas_bytes_to_client counter\natlas_bytes_to_client {}\n", self.requests_total.load(Ordering::Relaxed), self.requests_succeeded.load(Ordering::Relaxed), self.requests_failed.load(Ordering::Relaxed), self.upstream_failures.load(Ordering::Relaxed), self.upstream_timeouts.load(Ordering::Relaxed), self.bytes_to_client.load(Ordering::Relaxed)) }
}

#[derive(Debug, Clone)] struct BackendState { address: SocketAddr, unhealthy_until: Option<Instant> }
impl BackendState { fn healthy(&self, now: Instant) -> bool { self.unhealthy_until.is_none_or(|until| until <= now) } }
#[derive(Debug)] struct BackendPool { backends: Vec<BackendState>, next: usize }
impl BackendPool {
    fn new(backends: &[SocketAddr]) -> Self { Self { backends: backends.iter().copied().map(|address| BackendState { address, unhealthy_until: None }).collect(), next: 0 } }
    fn select(&mut self) -> Option<SocketAddr> { let now=Instant::now(); for _ in 0..self.backends.len() { let i=self.next%self.backends.len(); self.next=(self.next+1)%self.backends.len(); if self.backends[i].healthy(now) { return Some(self.backends[i].address); } } None }
    fn mark_failure(&mut self, address: SocketAddr) { if let Some(b)=self.backends.iter_mut().find(|b| b.address==address) { b.unhealthy_until=Some(Instant::now()+BACKEND_COOLDOWN); } }
    fn mark_success(&mut self, address: SocketAddr) { if let Some(b)=self.backends.iter_mut().find(|b| b.address==address) { b.unhealthy_until=None; } }
}

#[derive(Debug, Clone)]
pub struct ProxyState { config: ProxyConfig, pool: Arc<Mutex<BackendPool>>, connections: Arc<Mutex<HashMap<SocketAddr, Vec<TcpStream>>>>, metrics: Arc<ProxyMetrics> }
impl ProxyState {
    pub fn new(config: ProxyConfig) -> Self { Self { pool: Arc::new(Mutex::new(BackendPool::new(&config.backends))), connections: Arc::new(Mutex::new(HashMap::new())), config, metrics: Arc::new(ProxyMetrics::default()) } }
    pub fn metrics(&self) -> Arc<ProxyMetrics> { Arc::clone(&self.metrics) }
    pub fn backends(&self) -> &[SocketAddr] { &self.config.backends }
    pub async fn health_check(&self, backend: SocketAddr) -> bool { match timeout(self.config.connect_timeout, TcpStream::connect(backend)).await { Ok(Ok(stream)) => { drop(stream); self.pool.lock().await.mark_success(backend); true }, _ => { self.pool.lock().await.mark_failure(backend); false } } }
}

#[derive(Debug)]
pub enum ProxyError { Io(std::io::Error), Http(ParseError), InvalidHost, InvalidBackend(String), NoBackends, NoHealthyBackends, UpstreamUnavailable, UpstreamTimeout, RequestBodyTooLarge, ResponseBodyTooLarge, InvalidContentLength, InvalidConfiguration(&'static str) }
impl fmt::Display for ProxyError {
    fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result { match self { Self::Io(e)=>write!(f,"I/O error: {e}"),Self::Http(e)=>write!(f,"HTTP error: {e}"),Self::InvalidHost=>f.write_str("missing or invalid Host header"),Self::InvalidBackend(v)=>write!(f,"invalid backend address: {v}"),Self::NoBackends=>f.write_str("no upstream backends configured"),Self::NoHealthyBackends=>f.write_str("no healthy upstream backends available"),Self::UpstreamUnavailable=>f.write_str("upstream unavailable"),Self::UpstreamTimeout=>f.write_str("upstream request timed out"),Self::RequestBodyTooLarge=>f.write_str("request body exceeds maximum size"),Self::ResponseBodyTooLarge=>f.write_str("response body exceeds maximum size"),Self::InvalidContentLength=>f.write_str("invalid Content-Length header"),Self::InvalidConfiguration(n)=>write!(f,"invalid configuration: {n}") } }
}
impl std::error::Error for ProxyError {}
impl From<std::io::Error> for ProxyError { fn from(e:std::io::Error)->Self{Self::Io(e)} }
impl From<ParseError> for ProxyError { fn from(e:ParseError)->Self{Self::Http(e)} }

pub async fn proxy_connection(client: TcpStream) -> Result<(), ProxyError> { proxy_connection_with_state(client, ProxyState::new(ProxyConfig::default())).await }

pub async fn proxy_connection_with_state(mut client: TcpStream, state: ProxyState) -> Result<(), ProxyError> {
    let mut read_buffer=Vec::with_capacity(BUFFER_SIZE); let mut chunk=vec![0_u8;BUFFER_SIZE];
    loop {
        let header_end=loop { if let Some(end)=find_header_end(&read_buffer){break end;} let n=client.read(&mut chunk).await?; if n==0{return Ok(());} read_buffer.extend_from_slice(&chunk[..n]); if read_buffer.len()>MAX_HEADER_BYTES {state.metrics.request_failed(); return Err(ProxyError::Http(ParseError::HeadersTooLarge));} };
        let request=parse_request(&String::from_utf8_lossy(&read_buffer[..header_end]))?; state.metrics.request_started();
        if request.target=="/metrics" { let body=state.metrics.render_prometheus(); let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body); client.write_all(response.as_bytes()).await?; state.metrics.request_succeeded(response.len()); return Ok(()); }
        let content_length=request_content_length(&request)?; if content_length>MAX_BODY_BYTES {state.metrics.request_failed();return Err(ProxyError::RequestBodyTooLarge);} let total=header_end.checked_add(content_length).ok_or(ProxyError::RequestBodyTooLarge)?;
        while read_buffer.len()<total { let n=client.read(&mut chunk).await?; if n==0 {state.metrics.request_failed();return Err(ProxyError::Http(ParseError::IncompleteRequest));} read_buffer.extend_from_slice(&chunk[..n]); }
        let request_bytes=read_buffer[..total].to_vec();
        let response=match forward_with_retries(&state,&request,&request_bytes).await {Ok(r)=>r,Err(e)=>{state.metrics.request_failed();write_error_response(&mut client,&e).await?;return Err(e)}};
        let status=response_status(&response).unwrap_or(0); client.write_all(&response).await?; state.metrics.request_succeeded(response.len()); println!("access method={} target={} status={} bytes={}",request.method,request.target,status,response.len()); read_buffer.drain(..total);
        if request_has_connection_close(&request){client.shutdown().await?;return Ok(());}
    }
}

async fn forward_with_retries(state:&ProxyState,_request:&HttpRequest,request_bytes:&[u8])->Result<Vec<u8>,ProxyError>{ let attempts=state.config.max_retries.saturating_add(1); for _ in 0..attempts { let backend={state.pool.lock().await.select()}; let Some(backend)=backend else{return Err(ProxyError::NoHealthyBackends)}; match forward_once(state,backend,request_bytes).await {Ok(r)=>{state.pool.lock().await.mark_success(backend);return Ok(r)},Err(ProxyError::UpstreamTimeout)=>{state.metrics.upstream_timeout();state.pool.lock().await.mark_failure(backend)},Err(ProxyError::Io(_))|Err(ProxyError::UpstreamUnavailable)=>{state.metrics.upstream_failure();state.pool.lock().await.mark_failure(backend)},Err(e)=>return Err(e)}} Err(ProxyError::UpstreamUnavailable) }
async fn acquire_upstream(state:&ProxyState,backend:SocketAddr)->Result<TcpStream,ProxyError>{ if let Some(stream)=state.connections.lock().await.get_mut(&backend).and_then(Vec::pop){return Ok(stream)} match timeout(state.config.connect_timeout,TcpStream::connect(backend)).await {Ok(Ok(s))=>Ok(s),Ok(Err(_))=>Err(ProxyError::UpstreamUnavailable),Err(_)=>Err(ProxyError::UpstreamTimeout)} }
async fn release_upstream(state:&ProxyState,backend:SocketAddr,stream:TcpStream){state.connections.lock().await.entry(backend).or_default().push(stream);}
async fn forward_once(state:&ProxyState,backend:SocketAddr,request_bytes:&[u8])->Result<Vec<u8>,ProxyError>{ let mut upstream=acquire_upstream(state,backend).await?; upstream.set_nodelay(true)?; upstream.write_all(request_bytes).await?; match timeout(state.config.request_timeout,read_response(&mut upstream)).await {Ok(Ok((response,reusable)))=>{if reusable{release_upstream(state,backend,upstream).await;}Ok(response)},Ok(Err(e))=>Err(e),Err(_)=>Err(ProxyError::UpstreamTimeout)} }

async fn read_response(upstream:&mut TcpStream)->Result<(Vec<u8>,bool),ProxyError>{ let mut response=Vec::with_capacity(BUFFER_SIZE); let mut chunk=vec![0_u8;BUFFER_SIZE]; let header_end=loop{if let Some(e)=find_header_end(&response){break e;}let n=upstream.read(&mut chunk).await?;if n==0{return Err(ProxyError::UpstreamUnavailable)}response.extend_from_slice(&chunk[..n]);if response.len()>MAX_HEADER_BYTES&&find_header_end(&response).is_none(){return Err(ProxyError::Http(ParseError::HeadersTooLarge));}}; let head=parse_response_head(&response[..header_end])?; let reusable=response_reusable(&head); match head.body {ResponseBodyMode::None=>response.truncate(header_end),ResponseBodyMode::ContentLength(length)=>{if length>MAX_RESPONSE_BODY_BYTES{return Err(ProxyError::ResponseBodyTooLarge)}let total=header_end.checked_add(length).ok_or(ProxyError::ResponseBodyTooLarge)?;while response.len()<total{let n=upstream.read(&mut chunk).await?;if n==0{return Err(ProxyError::UpstreamUnavailable)}response.extend_from_slice(&chunk[..n]);}response.truncate(total)},ResponseBodyMode::Chunked=>read_chunked_body(upstream,&mut response).await?,ResponseBodyMode::UntilClose=>{upstream.read_to_end(&mut response).await?;if response.len().saturating_sub(header_end)>MAX_RESPONSE_BODY_BYTES{return Err(ProxyError::ResponseBodyTooLarge)}}} Ok((response,reusable&&!matches!(head.body,ResponseBodyMode::UntilClose))) }

async fn read_chunked_body(upstream:&mut TcpStream,response:&mut Vec<u8>)->Result<(),ProxyError>{let mut line=Vec::with_capacity(128);let mut total=0usize;loop{read_crlf_line(upstream,&mut line).await?;response.extend_from_slice(&line);let text=std::str::from_utf8(&line[..line.len().saturating_sub(2)]).map_err(|_|ProxyError::Http(ParseError::InvalidHeader))?;let size_text=text.split(';').next().unwrap_or_default().trim();let size=usize::from_str_radix(size_text,16).map_err(|_|ProxyError::Http(ParseError::InvalidContentLength))?;if size==0{loop{read_crlf_line(upstream,&mut line).await?;response.extend_from_slice(&line);if line==b"\r\n"{return Ok(())}}}total=total.checked_add(size).ok_or(ProxyError::ResponseBodyTooLarge)?;if total>MAX_RESPONSE_BODY_BYTES{return Err(ProxyError::ResponseBodyTooLarge)}let start=response.len();response.resize(start+size+2,0);upstream.read_exact(&mut response[start..start+size+2]).await?;if response[start+size..start+size+2]!=b"\r\n"{return Err(ProxyError::Http(ParseError::InvalidHeader))}}}
async fn read_crlf_line(upstream:&mut TcpStream,line:&mut Vec<u8>)->Result<(),ProxyError>{line.clear();loop{let mut byte=[0_u8;1];upstream.read_exact(&mut byte).await?;line.push(byte[0]);if line.len()>MAX_HEADER_BYTES{return Err(ProxyError::Http(ParseError::HeadersTooLarge))}if line.ends_with(b"\r\n"){return Ok(())}}}
fn response_reusable(head:&crate::http::HttpResponseHead)->bool{if head.headers.iter().any(|(n,v)|n.eq_ignore_ascii_case("connection")&&v.split(',').any(|t|t.trim().eq_ignore_ascii_case("close"))){return false}if head.version=="HTTP/1.0"{return head.headers.iter().any(|(n,v)|n.eq_ignore_ascii_case("connection")&&v.split(',').any(|t|t.trim().eq_ignore_ascii_case("keep-alive")))}true}
fn response_status(response:&[u8])->Option<u16>{let end=find_header_end(response)?;parse_response_head(&response[..end]).ok().map(|h|h.status)}
fn request_content_length(request:&HttpRequest)->Result<usize,ProxyError>{let Some((_,v))=request.headers.iter().find(|(n,_)|n.eq_ignore_ascii_case("content-length"))else{return Ok(0)};v.parse::<usize>().map_err(|_|ProxyError::InvalidContentLength)}
fn request_has_connection_close(request:&HttpRequest)->bool{request.headers.iter().any(|(n,v)|n.eq_ignore_ascii_case("connection")&&v.split(',').any(|t|t.trim().eq_ignore_ascii_case("close")))}
fn find_header_end(buffer:&[u8])->Option<usize>{buffer.windows(4).position(|w|w==b"\r\n\r\n").map(|p|p+4)}
async fn write_error_response(client:&mut TcpStream,error:&ProxyError)->Result<(),std::io::Error>{let(status,reason)=match error{ProxyError::Http(_)|ProxyError::InvalidContentLength|ProxyError::RequestBodyTooLarge|ProxyError::ResponseBodyTooLarge=>(400,"Bad Request"),ProxyError::UpstreamTimeout=>(504,"Gateway Timeout"),ProxyError::NoHealthyBackends|ProxyError::UpstreamUnavailable=>(503,"Service Unavailable"),_=>(502,"Bad Gateway")};let body=format!("ATLAS {status} {reason}\n");let response=format!("HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: text/plain\r\n\r\n{body}",body.len());client.write_all(response.as_bytes()).await}
fn parse_backends(value:&str)->Result<Vec<SocketAddr>,ProxyError>{value.split(',').map(str::trim).filter(|v|!v.is_empty()).map(|v|v.parse().map_err(|_|ProxyError::InvalidBackend(v.to_owned()))).collect()}
fn env_duration(name:&'static str,default:Duration)->Result<Duration,ProxyError>{let Some(v)=std::env::var(name).ok()else{return Ok(default)};let millis=v.parse::<u64>().map_err(|_|ProxyError::InvalidConfiguration(name))?;Ok(Duration::from_millis(millis))}

#[cfg(test)]
mod tests{use super::*;#[test]fn selects_backends_round_robin(){let a=vec![SocketAddr::from(([127,0,0,1],9000)),SocketAddr::from(([127,0,0,1],9001))];let mut p=BackendPool::new(&a);assert_eq!(p.select(),Some(a[0]));assert_eq!(p.select(),Some(a[1]));assert_eq!(p.select(),Some(a[0]));}#[test]fn failed_backend_is_temporarily_removed(){let a=SocketAddr::from(([127,0,0,1],9000));let mut p=BackendPool::new(&[a]);p.mark_failure(a);assert_eq!(p.select(),None)}#[test]fn successful_backend_is_restored(){let a=SocketAddr::from(([127,0,0,1],9000));let mut p=BackendPool::new(&[a]);p.mark_failure(a);p.mark_success(a);assert_eq!(p.select(),Some(a))}#[test]fn parses_backend_list(){assert_eq!(parse_backends("127.0.0.1:9000,127.0.0.1:9001").unwrap().len(),2)}#[test]fn response_reuse_defaults_to_keep_alive(){let h=parse_response_head(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n").unwrap();assert!(response_reusable(&h))}#[test]fn response_reuse_honors_close(){let h=parse_response_head(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n").unwrap();assert!(!response_reusable(&h))}}

#[cfg(test)]
mod response_runtime_tests{use super::*;use tokio::io::AsyncWriteExt;use tokio::net::TcpListener;#[tokio::test]async fn reads_content_length_response_without_close(){let l=TcpListener::bind("127.0.0.1:0").await.unwrap();let a=l.local_addr().unwrap();let server=tokio::spawn(async move{let(mut s,_)=l.accept().await.unwrap();s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: keep-alive\r\n\r\nhello").await.unwrap()});let mut c=TcpStream::connect(a).await.unwrap();let(r,reuse)=read_response(&mut c).await.unwrap();assert!(reuse);assert_eq!(r.last().copied(),Some(b'o'));server.await.unwrap()}#[tokio::test]async fn decodes_chunked_response(){let l=TcpListener::bind("127.0.0.1:0").await.unwrap();let a=l.local_addr().unwrap();let server=tokio::spawn(async move{let(mut s,_)=l.accept().await.unwrap();s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n").await.unwrap()});let mut c=TcpStream::connect(a).await.unwrap();let(r,reuse)=read_response(&mut c).await.unwrap();assert!(reuse);assert!(r.ends_with(b"0\r\n\r\n"));server.await.unwrap()}}
