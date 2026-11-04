/*
 * Development aid: a virtual D-pad gamepad on /dev/uinput that replays a
 * fixed navigation pattern, so UI rendering cost can be measured
 * reproducibly without a person pressing buttons. Never shipped.
 *
 * Usage: uinput-nav [pattern] [interval_ms] [repeat]
 *   pattern: R L U D (D-pad), A (south), B (east), N (north), W (west),
 *            S (start), E (select), M (mode: Quick Menu), + - (volume
 *            up/down), [ ] (L1/R1), Q (Mode + Start chord: quit),
 *            any other char = pause;
 *            default "RRRRLLLLDUDU"; "@" = persistent pad, sequences
 *            "PATTERN INTERVAL_MS" read from stdin, one per line"
 */
#include <fcntl.h>
#include <linux/uinput.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <time.h>
#include <unistd.h>

static int fd = -1;

static void emit(int type, int code, int value)
{
	struct input_event ev;

	memset(&ev, 0, sizeof(ev));
	ev.type = type;
	ev.code = code;
	ev.value = value;
	if (write(fd, &ev, sizeof(ev)) != sizeof(ev))
		perror("uinput-nav: write");
}

static void msleep(long ms)
{
	struct timespec ts = { ms / 1000, (ms % 1000) * 1000000L };

	nanosleep(&ts, NULL);
}

static void press(int code)
{
	emit(EV_KEY, code, 1);
	emit(EV_SYN, SYN_REPORT, 0);
	msleep(40);
	emit(EV_KEY, code, 0);
	emit(EV_SYN, SYN_REPORT, 0);
}

static void play(const char *pattern, long interval, int repeat)
{
	for (int r = 0; r < repeat; r++) {
		for (const char *p = pattern; *p; p++) {
			int code = *p == 'R' ? BTN_DPAD_RIGHT : *p == 'L' ? BTN_DPAD_LEFT :
				   *p == 'U' ? BTN_DPAD_UP : *p == 'D' ? BTN_DPAD_DOWN :
				   *p == 'A' ? BTN_SOUTH : *p == 'B' ? BTN_EAST :
				   *p == 'M' ? BTN_MODE : *p == '+' ? KEY_VOLUMEUP :
				   *p == '-' ? KEY_VOLUMEDOWN : *p == '[' ? BTN_TL :
				   *p == ']' ? BTN_TR : *p == 'N' ? BTN_NORTH :
				   *p == 'W' ? BTN_WEST : *p == 'S' ? BTN_START :
				   *p == 'E' ? BTN_SELECT : -1;
			if (*p == 'Q') {
				emit(EV_KEY, BTN_MODE, 1);
				emit(EV_SYN, SYN_REPORT, 0);
				msleep(80);
				press(BTN_START);
				msleep(80);
				emit(EV_KEY, BTN_MODE, 0);
				emit(EV_SYN, SYN_REPORT, 0);
			} else if (code >= 0)
				press(code);
			msleep(interval);
		}
	}
}

int main(int argc, char **argv)
{
	const char *pattern = argc > 1 ? argv[1] : "RRRRLLLLDUDU";
	long interval = argc > 2 ? atol(argv[2]) : 250;
	int repeat = argc > 3 ? atoi(argv[3]) : 1;
	static const int keys[] = { BTN_DPAD_UP, BTN_DPAD_DOWN, BTN_DPAD_LEFT,
				    BTN_DPAD_RIGHT, BTN_SOUTH, BTN_EAST, BTN_MODE,
				    KEY_VOLUMEUP, KEY_VOLUMEDOWN, BTN_TL, BTN_TR,
				    BTN_NORTH, BTN_WEST, BTN_START, BTN_SELECT };
	struct uinput_setup setup;
	size_t i;

	fd = open("/dev/uinput", O_WRONLY | O_NONBLOCK);
	if (fd < 0) {
		perror("uinput-nav: /dev/uinput");
		return 1;
	}
	ioctl(fd, UI_SET_EVBIT, EV_KEY);
	for (i = 0; i < sizeof(keys) / sizeof(keys[0]); i++)
		ioctl(fd, UI_SET_KEYBIT, keys[i]);

	memset(&setup, 0, sizeof(setup));
	setup.id.bustype = BUS_VIRTUAL;
	setup.id.vendor = 0x6e75;
	setup.id.product = 0x0001;
	snprintf(setup.name, UINPUT_MAX_NAME_SIZE, "nuubOS bench pad");
	ioctl(fd, UI_DEV_SETUP, &setup);
	if (ioctl(fd, UI_DEV_CREATE) < 0) {
		perror("uinput-nav: UI_DEV_CREATE");
		return 1;
	}
	/* Let inputd/controllersd pick the device up. */
	msleep(1500);

	if (strcmp(pattern, "@") == 0) {
		/* Persistent pad: one "PATTERN INTERVAL_MS" per stdin line, "done"
		 * after each, so a measurement does not pay a controller
		 * connection (toast, top bar) per sequence. */
		char line[1024], pat[1000];
		long iv;

		while (fgets(line, sizeof(line), stdin)) {
			if (sscanf(line, "%999s %ld", pat, &iv) != 2)
				continue;
			play(pat, iv, 1);
			printf("done\n");
			fflush(stdout);
		}
	} else {
		play(pattern, interval, repeat);
	}

	msleep(300);
	ioctl(fd, UI_DEV_DESTROY);
	close(fd);
	return 0;
}
