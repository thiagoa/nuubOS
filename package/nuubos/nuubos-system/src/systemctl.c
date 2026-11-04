#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#define SOCK "/run/nuubos/systemd.sock"

static int write_all(int fd, const void *buffer, size_t length)
{
    const unsigned char *p = buffer;

    while (length > 0) {
        ssize_t written = write(fd, p, length);
        if (written < 0) {
            if (errno == EINTR)
                continue;
            return -1;
        }
        if (written == 0) {
            errno = EIO;
            return -1;
        }
        p += (size_t)written;
        length -= (size_t)written;
    }

    return 0;
}

int main(int argc, char **argv)
{
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un address;

    if (fd < 0) {
        perror("socket");
        return 1;
    }

    memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    snprintf(address.sun_path, sizeof(address.sun_path), "%s", SOCK);

    if (connect(fd, (struct sockaddr *)&address, sizeof(address)) < 0) {
        perror("connect");
        close(fd);
        return 1;
    }

    if (argc < 2) {
        fprintf(stderr,
                "Usage: nuubos-systemctl status | set-profile <auto|battery-saver> | "
                "set-auto-battery <0-50> | set-charge-limit <80-100> | set-sleep <min> | "
                "set-hypersleep <0|1> | set-fast-startup <0|1> | set-power-key-sleep <0|1> | "
                "set-diagnostics <0-20 boots> | "
                "storage-changed | set-storage-backup <0|1> | set-storage-backup-item <item> <0|1> | "
                "set-storage-backup-dest <sd2|sd1|usb> | "
                "sd2 <eject|ignore|use> | storage-backup-archive | storage-data-forget <category> | "
                "sleep | restart | poweroff | activity | reset-system-settings\n");
        close(fd);
        return 2;
    }

    char command[256];

    if (!strcmp(argv[1], "status"))
        snprintf(command, sizeof(command), "STATUS\n");
    else if (!strcmp(argv[1], "set-profile") && argc == 3)
        snprintf(command, sizeof(command), "SET PROFILE %s\n", argv[2]);
    else if (!strcmp(argv[1], "set-auto-battery") && argc == 3)
        snprintf(command, sizeof(command), "SET AUTO_BATTERY %s\n", argv[2]);
    else if (!strcmp(argv[1], "set-charge-limit") && argc == 3)
        snprintf(command, sizeof(command), "SET CHARGE_LIMIT %s\n", argv[2]);
    else if (!strcmp(argv[1], "set-sleep") && argc == 3)
        snprintf(command, sizeof(command), "SET SLEEP %s\n", argv[2]);
    else if (!strcmp(argv[1], "set-diagnostics") && argc == 3)
        snprintf(command, sizeof(command), "SET DIAGNOSTICS %s\n", argv[2]);
    else if (!strcmp(argv[1], "set-hypersleep") && argc == 3)
        snprintf(command, sizeof(command), "SET HYPERSLEEP %s\n", argv[2]);
    else if (!strcmp(argv[1], "set-fast-startup") && argc == 3)
        snprintf(command, sizeof(command), "SET FAST_STARTUP %s\n", argv[2]);
    else if (!strcmp(argv[1], "set-power-key-sleep") && argc == 3)
        snprintf(command, sizeof(command), "SET POWER_KEY_SLEEP %s\n", argv[2]);
    else if (!strcmp(argv[1], "storage-changed") && argc == 2)
        snprintf(command, sizeof(command), "STORAGE CHANGED\n");
    else if (!strcmp(argv[1], "set-storage-backup") && argc == 3)
        snprintf(command, sizeof(command), "SET STORAGE BACKUP %s\n", argv[2]);
    else if (!strcmp(argv[1], "set-storage-backup-dest") && argc == 3)
        snprintf(command, sizeof(command), "SET STORAGE BACKUP_DEST %s\n", argv[2]);
    else if (!strcmp(argv[1], "set-storage-backup-item") && argc == 4)
        snprintf(command, sizeof(command), "SET STORAGE BACKUP_ITEM %s %s\n", argv[2], argv[3]);
    else if (!strcmp(argv[1], "sd2") && argc == 3 &&
             (!strcmp(argv[2], "eject") || !strcmp(argv[2], "ignore") || !strcmp(argv[2], "use"))) {
        char value[16];
        snprintf(value, sizeof(value), "%s", argv[2]);
        for (char *p = value; *p; p++)
            *p = (char)(*p - 'a' + 'A');
        snprintf(command, sizeof(command), "STORAGE SD2 %s\n", value);
    } else if (!strcmp(argv[1], "storage-data-forget") && argc == 3)
        snprintf(command, sizeof(command), "STORAGE DATA FORGET %.15s\n", argv[2]);
    else if (!strcmp(argv[1], "storage-backup-archive") && argc == 2)
        snprintf(command, sizeof(command), "STORAGE BACKUP ARCHIVE\n");
    else if (!strcmp(argv[1], "sleep") && argc == 2)
        snprintf(command, sizeof(command), "ACTION SLEEP\n");
    else if (!strcmp(argv[1], "restart") && argc == 2)
        snprintf(command, sizeof(command), "ACTION RESTART\n");
    else if (!strcmp(argv[1], "poweroff") && argc == 2)
        snprintf(command, sizeof(command), "ACTION POWEROFF\n");
    else if (!strcmp(argv[1], "activity") && argc == 2)
        snprintf(command, sizeof(command), "ACTIVITY\n");
    else if (!strcmp(argv[1], "reset-system-settings"))
        snprintf(command, sizeof(command), "RESET SYSTEM SETTINGS\n");
    else {
        fprintf(stderr, "invalid command\n");
        close(fd);
        return 2;
    }

    if (write_all(fd, command, strlen(command)) < 0) {
        perror("write systemd command");
        close(fd);
        return 1;
    }

    /* storage-changed comes from the storage tools, which nuubos-systemd
     * itself may be running and waiting for: never wait for its reply. */
    if (!strcmp(argv[1], "storage-changed")) {
        close(fd);
        return 0;
    }

    if (shutdown(fd, SHUT_WR) < 0) {
        perror("shutdown");
        close(fd);
        return 1;
    }

    char buffer[1024];
    ssize_t count;

    while ((count = read(fd, buffer, sizeof(buffer))) > 0) {
        if (write_all(STDOUT_FILENO, buffer, (size_t)count) < 0) {
            perror("write stdout");
            close(fd);
            return 1;
        }
    }

    if (count < 0) {
        perror("read systemd reply");
        close(fd);
        return 1;
    }

    close(fd);
    return 0;
}
