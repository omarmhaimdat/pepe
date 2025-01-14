# Pepe - HTTP Load Generator

Pepe is a command-line HTTP load generator designed to test the performance and reliability of web servers. It allows you to send a large number of HTTP requests to a specified URL and measure various performance metrics such as response times, throughput, and error rates.

Pepe is written in Rust and uses the `reqwest` and `tokio` libraries for making HTTP requests asynchronously. It supports sending multiple requests concurrently, custom headers, request bodies, timeouts, basic authentication, and proxy servers.

![Pepe](assets/pepe.gif)

## Features

- **Concurrency**: Send multiple requests concurrently to simulate real-world load.
- **Custom Headers**: Add custom headers to the requests.
- **Request Body**: Send data in the request body from a string or a file.
- **Timeouts**: Set a timeout for each request.
- **Basic Authentication**: Use basic authentication for the requests.
- **Proxy Support**: Send requests through a proxy server.
- **DNS Resolution Timing**: Measure DNS lookup and resolution times.
- **Detailed Statistics**: Measure and display various performance metrics such as min, max, average, median, percentiles, standard deviation, total data transferred, and error rate.

## Installation

### Linux and MacOS (Shell Script)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://pepe.mhaimdat.com/install.sh | sh
```

### MacOS Package (Homebrew)

```bash
brew tap omarmhaimdat/tap
brew install pepe
```

### Manual Installation

Make sure you have Rust installed on your system. You can install Rust using `rustup` by following the instructions on the [official website](https://www.rust-lang.org/tools/install).

Once you have Rust installed, you can build and install Pepe using Cargo, the Rust package manager:


Clone the repository:

```bash
git clone https://github.com/omarmhaimdat/pepe.git
```

Change to the project directory:
```bash
cd pepe
```

Build and install the Pepe binary using Cargo:
```bash
cargo install --path .
```

This will build the Pepe binary and install it in your Cargo bin directory, which should be in your system's PATH.

## Usage

### Basic Usage

To send a simple GET request to a URL, use the following command:

```bash
pepe https://example.com
```

### Advanced Usage

```bash
pepe -n 1000 -c 20 -t 10 -u "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_11_2) AppleWebKit/601.3.9 (KHTML, like Gecko) Version/9.0.2 Safari/601.3.9" -H "Accept: application/json" -H "Content-Type: application/json" -m GET https://example.com
```

Let's break down the options used in this command:

- `-n 1000`: Send a total of 1000 requests.
- `-c 20`: Use 20 concurrent connections.
- `-t 10`: Set a timeout of 10 seconds for each request.
- `-u "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_11_2) AppleWebKit/601.3.9 (KHTML, like Gecko) Version/9.0.2 Safari/601.3.9"`: Set the User-Agent header to simulate a Safari browser on a Mac.
- `-H "Accept: application/json"`: Add a custom Accept header to the requests.
- `-H "Content-Type: application/json"`: Add a custom Content-Type header to the requests.
- `-m GET`: Use the GET HTTP method.
- `https://example.com`: The URL to send requests to.


## Examples

### Sending a GET Request

```bash
pepe -n 1000 -c 10 -m GET https://example.com
```

This command sends 1000 GET requests to `https://example.com` with a concurrency of 10 requests at a time, -m GET specifies the HTTP method to use.

### Sending a POST Request

Send a POST request with a request body as raw text:

```bash
pepe -n 1000 -c 10 -m POST -d 'Hello, World!' https://httpbin.org/post
```

Send a POST request with a request body in json format:

```bash
pepe -n 1000 -c 10 -m POST -d '{"key": "value"}' -H 'Content-Type: application/json' https://httpbin.org/post
```

### Sending Requests with Custom Headers

```bash
pepe -n 100 -c 5 -H "User-Agent: Pepe/1.0" -H "X-Custom-Header: Value" https://example.com
```

### Proxy Support

Send requests through a proxy server (HTTP or HTTPS, SOCKS5):

Without authentication:

```bash
pepe -n 1000 -c 10 -x http://proxy:port https://example.com
```


With authentication:

```bash
pepe -n 1000 -c 10 -x socks5://username:password@proxy:port https://example.com
```

## Output

Pepe provides detailed statistics about the performance of the web server, including:

- **Min Response Time**: The minimum response time observed.
- **Max Response Time**: The maximum response time observed.
- **Average Response Time**: The average response time.
- **Median Response Time**: The median response time.
- **90th Percentile**: The 90th percentile response time.
- **95th Percentile**: The 95th percentile response time.
- **99th Percentile**: The 99th percentile response time.
- **Standard Deviation**: The standard deviation of the response times.
- **Total Data Transferred**: The total amount of data transferred.
- **Error Rate**: The percentage of requests that resulted in errors.
- **Cache Hit Rate**: The percentage of requests that were served from the cache.
- **Requests Per Second (RPS)**: The number of requests per second.
- **DNS Lookup Time**: The time taken to resolve the DNS.
- **DNS Resolution Time**: The time taken to resolve the DNS addresses.


## Contributing

Contributions are welcome! Please open an issue or submit a pull request on GitHub.

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

## Acknowledgements

- [Clap](https://github.com/clap-rs/clap) for command-line argument parsing.
- [Reqwest](https://github.com/seanmonstar/reqwest) for HTTP requests.
- [Tokio](https://github.com/tokio-rs/tokio) for asynchronous runtime.
- [Crossterm](https://github.com/crossterm-rs/crossterm) for terminal handling.
