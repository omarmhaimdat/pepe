# Pepe - HTTP Load Generator

Pepe is a command-line HTTP load generator designed to test the performance and reliability of web servers. It allows you to send a large number of HTTP requests to a specified URL and measure various performance metrics such as response times, throughput, and error rates.

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

### Debian Package (Ubuntu, Debian, etc.)

### MacOS Package (Homebrew)

### Manual Installation

## Usage

### Basic Usage

### Advanced Usage

## Examples

### Sending a GET Request

### Sending a POST Request

### Sending Requests with Custom Headers

### Sending Requests with a Request Body

### Basic Authentication

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
