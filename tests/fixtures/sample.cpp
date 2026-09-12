#include <string>
/* C++ raw string demo */
int main() {
    // greeting line
    auto s = R"delim(line // not comment
/* still string */)delim";
    return 0;
}
