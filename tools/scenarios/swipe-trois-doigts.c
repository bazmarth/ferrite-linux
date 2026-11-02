/* Faux pavé tactile (uinput) qui fait glisser trois doigts, pour tester le geste dans la
 * machine virtuelle : ses événements passent par le vrai libinput.
 *
 *   gcc -static -nostdlib -O2 -o build/swipe tools/scenarios/swipe-trois-doigts.c
 *   swipe COURSE ATTENTE_MS   course en unités du pavé (12 par mm), négative vers la gauche ;
 *                             les doigts restent posés ATTENTE_MS avant de se lever.
 *
 * Sans libc : le binaire doit tourner dans l'image Debian quel que soit le système hôte. */
#include <linux/input.h>
#include <linux/uinput.h>

static long sys(long n, long a, long b, long c) {
    long r;
    __asm__ volatile("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c) : "rcx", "r11", "memory");
    return r;
}
static int fd;
static void sleep_ms(long ms) {
    long t[2] = {ms / 1000, (ms % 1000) * 1000000};
    sys(35, (long)t, 0, 0);
}
static void ctl(unsigned long req, long arg) { sys(16, fd, req, arg); }
static void emit(int type, int code, int value) {
    struct input_event ev = {.type = type, .code = code, .value = value};
    sys(1, fd, (long)&ev, sizeof ev);
}
static void axis(int code, int min, int max, int res) {
    struct uinput_abs_setup abs = {.code = code};
    abs.absinfo.minimum = min;
    abs.absinfo.maximum = max;
    abs.absinfo.resolution = res;
    ctl(UI_ABS_SETUP, (long)&abs);
}
static int number(const char *s) {
    int sign = 1, n = 0;
    if (*s == '-') { sign = -1; s++; }
    while (*s) n = n * 10 + *s++ - '0';
    return sign * n;
}
#define FINGERS 3
static void place(int x) {
    for (int f = 0; f < FINGERS; f++) {
        emit(EV_ABS, ABS_MT_SLOT, f);
        emit(EV_ABS, ABS_MT_POSITION_X, x + f * 180);
        emit(EV_ABS, ABS_MT_POSITION_Y, 400);
    }
    emit(EV_ABS, ABS_X, x);
    emit(EV_ABS, ABS_Y, 400);
    emit(EV_SYN, SYN_REPORT, 0);
}
static void run(int travel, int hold) {
    fd = sys(2, (long)"/dev/uinput", 2 /* O_RDWR */, 0);
    if (fd < 0) sys(60, 2, 0, 0);
    ctl(UI_SET_EVBIT, EV_KEY);
    ctl(UI_SET_EVBIT, EV_ABS);
    ctl(UI_SET_PROPBIT, INPUT_PROP_POINTER);
    ctl(UI_SET_KEYBIT, BTN_LEFT);
    ctl(UI_SET_KEYBIT, BTN_TOUCH);
    ctl(UI_SET_KEYBIT, BTN_TOOL_FINGER);
    ctl(UI_SET_KEYBIT, BTN_TOOL_DOUBLETAP);
    ctl(UI_SET_KEYBIT, BTN_TOOL_TRIPLETAP);
    int absolutes[] = {ABS_X, ABS_Y, ABS_MT_SLOT, ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_TRACKING_ID};
    for (int i = 0; i < 6; i++) ctl(UI_SET_ABSBIT, absolutes[i]);
    axis(ABS_X, 0, 1200, 12);
    axis(ABS_Y, 0, 800, 12);
    axis(ABS_MT_POSITION_X, 0, 1200, 12);
    axis(ABS_MT_POSITION_Y, 0, 800, 12);
    axis(ABS_MT_SLOT, 0, 4, 0);
    axis(ABS_MT_TRACKING_ID, 0, 65535, 0);
    struct uinput_setup setup = {.id = {.bustype = BUS_VIRTUAL, .vendor = 0x1234, .product = 0x5678}};
    const char *name = "Ferrite Test Touchpad";
    for (int i = 0; name[i]; i++) setup.name[i] = name[i];
    ctl(UI_DEV_SETUP, (long)&setup);
    ctl(UI_DEV_CREATE, 0);
    sleep_ms(1500); /* Le temps que libinput ouvre le périphérique. */

    int x = travel < 0 ? 800 : 150;
    for (int f = 0; f < FINGERS; f++) {
        emit(EV_ABS, ABS_MT_SLOT, f);
        emit(EV_ABS, ABS_MT_TRACKING_ID, 100 + f);
    }
    emit(EV_KEY, BTN_TOUCH, 1);
    emit(EV_KEY, BTN_TOOL_TRIPLETAP, 1);
    place(x);
    sleep_ms(50);
    for (int step = 1; step <= 25; step++) {
        place(x + travel * step / 25);
        sleep_ms(12);
    }
    sleep_ms(hold);
    for (int f = 0; f < FINGERS; f++) {
        emit(EV_ABS, ABS_MT_SLOT, f);
        emit(EV_ABS, ABS_MT_TRACKING_ID, -1);
    }
    emit(EV_KEY, BTN_TOUCH, 0);
    emit(EV_KEY, BTN_TOOL_TRIPLETAP, 0);
    emit(EV_SYN, SYN_REPORT, 0);
    sleep_ms(500);
    ctl(UI_DEV_DESTROY, 0);
}
__attribute__((used)) void start_c(long *stack) {
    char **argv = (char **)(stack + 1);
    if (stack[0] == 3) run(number(argv[1]), number(argv[2]));
    sys(60, stack[0] == 3 ? 0 : 1, 0, 0);
}
__asm__(".globl _start\n_start:\n mov %rsp,%rdi\n and $-16,%rsp\n call start_c\n");
