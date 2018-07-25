#include <iostream>
#include <string.h>
#include <netinet/in.h>
#include <assert.h>

#include "../include/io-queue.h"

#define USE_CONNECT		0

uint16_t port = 12345;

int main()
{
	errno = 0;
    int qd;
    ssize_t n;
    Zeus::qtoken qt;
    struct Zeus::sgarray sga, res;
    char buf[12] = "hello world";
    struct sockaddr_in server;

    char* argv[] = {(char*)"",
                    (char*)"-b",
                    (char*)"0000:03:00.0",
                    (char*)"-l",
                    (char*)"1",
                    (char*)"-m",
                    (char*)"256",
                    (char*)"--no-shconf",
                    (char*)"--file-prefix",
                    (char*)"c" };
    int argc = 10;

    if (Zeus::init(argc, argv) < 0) {
        printf("Error initializing Zeus!\n");
        return -1;
    }

    if ((qd = Zeus::queue(AF_INET, SOCK_DGRAM, 0)) < 0) {
        printf("Error creating queue!\n");
        return -1;
    }

    printf("client qd:\t%d\n", qd);

    server.sin_family = AF_INET;
    server.sin_addr.s_addr = htonl(0x0c0c0c04);
    server.sin_port = htons(port);

#if USE_CONNECT
    if (Zeus::connect(qd, (struct sockaddr*)&server, sizeof(server)) < 0) {
    	printf("Error connecting queue!\n");
    	return -1;
    }
#else
    sga.addr = server;
#endif

    sga.num_bufs = 1;
    sga.bufs[0].len = 12;
    sga.bufs[0].buf = (Zeus::ioptr)buf;

	//n = Zeus::blocking_push(qd, sga);
	//assert(n > 0);
	qt = Zeus::push(qd, sga);
	if (qt != 0) {
		printf("client wait for push\n");
		n = Zeus::wait(qt, sga);
		assert(n > 0);
	}

	printf("client: sent\t%s\n", (char*)sga.bufs[0].buf);

	qt = Zeus::pop(qd, res);
	if (qt != 0) {
		printf("client: wait for pop\n");
		n = Zeus::wait(qt, res);
		assert(n > 0);
	}

	assert(res.num_bufs == 1);
	printf("client: rcvd\t%s\n", (char*)res.bufs[0].buf);

    Zeus::close(qd);

    return 0;
}
