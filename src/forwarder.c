/*
 *  https://github.com/jamesbarlow/icmptunnel
 *
 *  The MIT License (MIT)
 *
 *  Copyright (c) 2016 James Barlow-Bignell
 *
 *  Permission is hereby granted, free of charge, to any person obtaining a copy
 *  of this software and associated documentation files (the "Software"), to deal
 *  in the Software without restriction, including without limitation the rights
 *  to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 *  copies of the Software, and to permit persons to whom the Software is
 *  furnished to do so, subject to the following conditions:
 *
 *  The above copyright notice and this permission notice shall be included in all
 *  copies or substantial portions of the Software.
 *
 *  THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 *  IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 *  FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 *  AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 *  LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 *  OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 *  SOFTWARE.
 */

#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/time.h>
#include <sys/select.h>

#include "config.h"
#include "options.h"
#include "peer.h"
#include "handlers.h"
#include "echo-skt.h"
#include "tun-device.h"
#include "forwarder.h"

/* are we still running? */
static int running = 1;

int forward(struct peer *peer, const struct handlers *handlers)
{
    struct echo_skt *skt = &peer->skt;
    struct tun_device *device = &peer->device;
    int maxfd = skt->fd > device->fd ? skt->fd : device->fd;

    /* loop and push packets between the tunnel device and peer. */
    while (running) {
        struct timeval timeout;
        int ret;
        fd_set fs;

        /* fill fd set */
        FD_ZERO(&fs);
        FD_SET(skt->fd, &fs);
        FD_SET(device->fd, &fs);

        /* set the timeout. */
        timeout.tv_sec = ICMPTUNNEL_PUNCHTHRU_INTERVAL;
        timeout.tv_usec = 0;

        /* wait for some data. */
        ret = select(maxfd + 1, &fs, NULL, NULL, &timeout);

        if (ret < 0) {
            if (!running)
                break;
            fprintf(stderr, "unable to select() on fds: %s\n", strerror(errno));
            return -1;
        }
        /* did we time out? */
        if (ret == 0) {
            handlers->timeout(peer);
            continue;
        }

        /* handle a packet from the echo socket. */
        if (FD_ISSET(skt->fd, &fs)) {
            handlers->icmp(peer);
        }

        /* handle data from the tunnel device. */
        if (FD_ISSET(device->fd, &fs)) {
            handlers->tunnel(peer);
        }
    }

    return 0;
}

void stop()
{
    running = 0;
}
