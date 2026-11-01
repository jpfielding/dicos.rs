// Reference decoder used only by the opt-in CharLS interoperability test.
#include <charls/charls_jpegls_decoder.h>
#include <fstream>
#include <iostream>
#include <iterator>
#include <vector>

int main(int argc, char** argv) {
    if (argc != 3) return 2;
    try {
        std::ifstream input(argv[1], std::ios::binary);
        if (!input) return 3;
        const std::vector<unsigned char> source{std::istreambuf_iterator<char>{input}, {}};
        std::vector<unsigned char> pixels;
        charls::jpegls_decoder::decode(source, pixels);
        std::ofstream output(argv[2], std::ios::binary);
        output.write(reinterpret_cast<const char*>(pixels.data()), pixels.size());
        return output ? 0 : 4;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
