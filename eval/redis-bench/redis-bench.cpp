#include <iostream>
#include <string>
#include <thread>
#include <vector>
#include <chrono>
#include <asio.hpp>
#include <asio/ssl.hpp>
#include <atomic>
#include <map>
#include <mutex>
#include <pthread.h>

using asio::ip::tcp;

std::atomic<int> total_requests(0);
std::atomic<int> total_failures(0);
std::atomic<bool> stop_benchmark(false);

void pin_thread_to_core(int core_id) {
    cpu_set_t cpuset;
    CPU_ZERO(&cpuset);
    CPU_SET(core_id, &cpuset);

    pthread_t current_thread = pthread_self();
    int rc = pthread_setaffinity_np(current_thread, sizeof(cpu_set_t), &cpuset);
    if (rc != 0) {
        std::cerr << "Error calling pthread_setaffinity_np: " << rc << "\n";
    }
}

void send_redis_get_request(asio::ssl::stream<tcp::socket>& ssl_socket, const std::string& key) {
    std::string request = "*2\r\n$3\r\nGET\r\n$" + std::to_string(key.length()) + "\r\n" + key + "\r\n";
    asio::write(ssl_socket, asio::buffer(request));
    
    asio::streambuf response;
    asio::read_until(ssl_socket, response, "\r\n");

    std::string line;
    std::istream response_stream(&response);
    std::getline(response_stream, line);
}

void make_requests(int connection_id, const std::string &host, const std::string &port, const std::string &backup_host, const std::string &backup_port, const std::string& redis_key, std::vector<std::chrono::high_resolution_clock::time_point>& timestamps) {
    try {
        pin_thread_to_core(connection_id % 16);

        asio::io_context io_context;
        asio::ssl::context ssl_context(asio::ssl::context::tlsv12_client);

        // Load client certificate
        ssl_context.use_certificate_chain_file("/usr/local/tls/svr.crt");

        // Load private key
        ssl_context.use_private_key_file("/usr/local/tls/svr.key", asio::ssl::context::pem);

        // Load CA certificate to verify the server
        ssl_context.load_verify_file("/usr/local/tls/CA.pem");

        // Set verify mode to require a certificate
        ssl_context.set_verify_mode(asio::ssl::verify_peer);

        tcp::resolver resolver(io_context);
        asio::ssl::stream<tcp::socket> ssl_socket(io_context, ssl_context);

        auto endpoints = resolver.resolve(host, port);
        asio::error_code ec;
        asio::connect(ssl_socket.lowest_layer(), endpoints, ec);

        if (ec) {
            std::cerr << "Connection failed: " << ec.message() << "\n";
            return;
        }

        // Perform SSL handshake
        ssl_socket.handshake(asio::ssl::stream_base::client);

        // Add a 10ms delay before sending the first request
        std::this_thread::sleep_for(std::chrono::milliseconds(10));

        while (!stop_benchmark.load()) {
            auto start_time = std::chrono::high_resolution_clock::now();

            send_redis_get_request(ssl_socket, redis_key);

            auto end_time = std::chrono::high_resolution_clock::now();
            timestamps.push_back(end_time);

            total_requests++;
        }
    } catch (const std::exception &e) {
        total_failures++;
        if (!backup_host.empty()) {
            std::cerr << "Connection " << connection_id << " - Switching to backup server " << backup_host << ":" << backup_port << "\n";
            make_requests(connection_id, backup_host, backup_port, "", "", redis_key, timestamps);
        }
    }
}

int main(int argc, char *argv[]) {
    std::string host, port, backup_host, backup_port, redis_key = "default_key";
    int num_connections = 1;
    int runtime_seconds = 10;

    for (int i = 1; i < argc; ++i) {
        std::string arg = argv[i];
        if (arg == "-h" && i + 1 < argc) {
            host = argv[++i];
        } else if (arg == "-p" && i + 1 < argc) {
            port = argv[++i];
        } else if (arg == "-c" && i + 1 < argc) {
            num_connections = std::stoi(argv[++i]);
        } else if (arg == "--backup-host" && i + 1 < argc) {
            backup_host = argv[++i];
        } else if (arg == "--backup-port" && i + 1 < argc) {
            backup_port = argv[++i];
        } else if (arg == "--redis-key" && i + 1 < argc) {
            redis_key = argv[++i];
        } else if (arg == "-t" && i + 1 < argc) {
            runtime_seconds = std::stoi(argv[++i]);
        }
    }

    if (host.empty() || port.empty()) {
        std::cerr << "Usage: " << argv[0] << " -h <host> -p <port> -c <num_connections> -t <runtime_seconds> [--backup-host <backup_host>] [--backup-port <backup_port>] [--redis-key <key>]\n";
        return 1;
    }

    std::vector<std::thread> threads;
    std::vector<std::vector<std::chrono::high_resolution_clock::time_point>> thread_timestamps(num_connections);

    for (int i = 0; i < num_connections; ++i) {
        threads.emplace_back(std::thread(make_requests, i + 1, host, port, backup_host, backup_port, redis_key, std::ref(thread_timestamps[i])));
    }

    // Run the benchmark for the specified amount of time
    std::this_thread::sleep_for(std::chrono::seconds(runtime_seconds));
    stop_benchmark.store(true);

    for (auto &t : threads) {
        t.join();
    }

    std::cout << "Total Requests: " << total_requests.load() << "\n";
    std::cout << "Total Failures: " << total_failures.load() << "\n";

    // Aggregate and analyze results
    std::map<long long, int> requests_per_ms;

    // Find the minimum start_time across all threads
    auto start_time = thread_timestamps[0].front();
    for (const auto& timestamps : thread_timestamps) {
        if (!timestamps.empty()) {
            start_time = std::min(start_time, timestamps.front());
        }
    }

    // Merge all timestamps into a single map
    for (const auto& timestamps : thread_timestamps) {
        for (const auto& timestamp : timestamps) {
            auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(timestamp - start_time).count();
            requests_per_ms[ms / 10]++;
        }
    }

    // Print out the results
    for (const auto& entry : requests_per_ms) {
        std::cout << entry.first << "," << entry.second << "\n";
    }

    return 0;
}
