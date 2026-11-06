//! System queries for the `نظام` native module
//! (catalog task 14, from `arch/*/entry/syscalls`): the
//! syscall-name table for this architecture plus read-only
//! identity queries (kernel version, OS name, uid, gid).
//! One implementation, shared by both engines (interpreter
//! + VM) through the stdlib native functions — never
//! implemented twice.
//!
//! Permission design (written before the code — see
//! `CATALOG-06-SYSTEM.md` §1): read-only only. No raw
//! syscall is ever executed from Niva code — there is no
//! `نادِ`; the table is data (names + numbers from the
//! Linux kernel tables), and the queries read /proc files
//! on Linux (+Android). Everywhere else they refuse with
//! an honest Arabic error — no invented value, no crash.
//! No new dependencies (no libc): everything here is std
//! plus /proc reads.

use crate::error::{RioError, RioResult};

/// The WASM refusal: the flat builtins' own message.
#[cfg(target_arch = "wasm32")]
const WASM_REFUSAL: &str = "هذه الدالة غير مدعومة في متصفح الويب";

/// The syscall table for this architecture: (number, name),
/// sorted by number, no duplicates. Generated from the Linux
/// kernel tables (common + 64-bit ABI only; x32/time32 and
/// other architectures' entries excluded — see
/// CATALOG-06-SYSTEM.md §1). Linux + Android only: on
/// macOS/Windows the Linux numbers mean other syscalls or
/// nothing, so returning them there would be misleading.
#[cfg(all(any(target_os = "linux", target_os = "android"), target_arch = "x86_64"))]
static SYSCALLS: &[(u32, &str)] = &[
    (0, "read"),
    (1, "write"),
    (2, "open"),
    (3, "close"),
    (4, "stat"),
    (5, "fstat"),
    (6, "lstat"),
    (7, "poll"),
    (8, "lseek"),
    (9, "mmap"),
    (10, "mprotect"),
    (11, "munmap"),
    (12, "brk"),
    (13, "rt_sigaction"),
    (14, "rt_sigprocmask"),
    (15, "rt_sigreturn"),
    (16, "ioctl"),
    (17, "pread64"),
    (18, "pwrite64"),
    (19, "readv"),
    (20, "writev"),
    (21, "access"),
    (22, "pipe"),
    (23, "select"),
    (24, "sched_yield"),
    (25, "mremap"),
    (26, "msync"),
    (27, "mincore"),
    (28, "madvise"),
    (29, "shmget"),
    (30, "shmat"),
    (31, "shmctl"),
    (32, "dup"),
    (33, "dup2"),
    (34, "pause"),
    (35, "nanosleep"),
    (36, "getitimer"),
    (37, "alarm"),
    (38, "setitimer"),
    (39, "getpid"),
    (40, "sendfile"),
    (41, "socket"),
    (42, "connect"),
    (43, "accept"),
    (44, "sendto"),
    (45, "recvfrom"),
    (46, "sendmsg"),
    (47, "recvmsg"),
    (48, "shutdown"),
    (49, "bind"),
    (50, "listen"),
    (51, "getsockname"),
    (52, "getpeername"),
    (53, "socketpair"),
    (54, "setsockopt"),
    (55, "getsockopt"),
    (56, "clone"),
    (57, "fork"),
    (58, "vfork"),
    (59, "execve"),
    (60, "exit"),
    (61, "wait4"),
    (62, "kill"),
    (63, "uname"),
    (64, "semget"),
    (65, "semop"),
    (66, "semctl"),
    (67, "shmdt"),
    (68, "msgget"),
    (69, "msgsnd"),
    (70, "msgrcv"),
    (71, "msgctl"),
    (72, "fcntl"),
    (73, "flock"),
    (74, "fsync"),
    (75, "fdatasync"),
    (76, "truncate"),
    (77, "ftruncate"),
    (78, "getdents"),
    (79, "getcwd"),
    (80, "chdir"),
    (81, "fchdir"),
    (82, "rename"),
    (83, "mkdir"),
    (84, "rmdir"),
    (85, "creat"),
    (86, "link"),
    (87, "unlink"),
    (88, "symlink"),
    (89, "readlink"),
    (90, "chmod"),
    (91, "fchmod"),
    (92, "chown"),
    (93, "fchown"),
    (94, "lchown"),
    (95, "umask"),
    (96, "gettimeofday"),
    (97, "getrlimit"),
    (98, "getrusage"),
    (99, "sysinfo"),
    (100, "times"),
    (101, "ptrace"),
    (102, "getuid"),
    (103, "syslog"),
    (104, "getgid"),
    (105, "setuid"),
    (106, "setgid"),
    (107, "geteuid"),
    (108, "getegid"),
    (109, "setpgid"),
    (110, "getppid"),
    (111, "getpgrp"),
    (112, "setsid"),
    (113, "setreuid"),
    (114, "setregid"),
    (115, "getgroups"),
    (116, "setgroups"),
    (117, "setresuid"),
    (118, "getresuid"),
    (119, "setresgid"),
    (120, "getresgid"),
    (121, "getpgid"),
    (122, "setfsuid"),
    (123, "setfsgid"),
    (124, "getsid"),
    (125, "capget"),
    (126, "capset"),
    (127, "rt_sigpending"),
    (128, "rt_sigtimedwait"),
    (129, "rt_sigqueueinfo"),
    (130, "rt_sigsuspend"),
    (131, "sigaltstack"),
    (132, "utime"),
    (133, "mknod"),
    (134, "uselib"),
    (135, "personality"),
    (136, "ustat"),
    (137, "statfs"),
    (138, "fstatfs"),
    (139, "sysfs"),
    (140, "getpriority"),
    (141, "setpriority"),
    (142, "sched_setparam"),
    (143, "sched_getparam"),
    (144, "sched_setscheduler"),
    (145, "sched_getscheduler"),
    (146, "sched_get_priority_max"),
    (147, "sched_get_priority_min"),
    (148, "sched_rr_get_interval"),
    (149, "mlock"),
    (150, "munlock"),
    (151, "mlockall"),
    (152, "munlockall"),
    (153, "vhangup"),
    (154, "modify_ldt"),
    (155, "pivot_root"),
    (156, "_sysctl"),
    (157, "prctl"),
    (158, "arch_prctl"),
    (159, "adjtimex"),
    (160, "setrlimit"),
    (161, "chroot"),
    (162, "sync"),
    (163, "acct"),
    (164, "settimeofday"),
    (165, "mount"),
    (166, "umount2"),
    (167, "swapon"),
    (168, "swapoff"),
    (169, "reboot"),
    (170, "sethostname"),
    (171, "setdomainname"),
    (172, "iopl"),
    (173, "ioperm"),
    (174, "create_module"),
    (175, "init_module"),
    (176, "delete_module"),
    (177, "get_kernel_syms"),
    (178, "query_module"),
    (179, "quotactl"),
    (180, "nfsservctl"),
    (181, "getpmsg"),
    (182, "putpmsg"),
    (183, "afs_syscall"),
    (184, "tuxcall"),
    (185, "security"),
    (186, "gettid"),
    (187, "readahead"),
    (188, "setxattr"),
    (189, "lsetxattr"),
    (190, "fsetxattr"),
    (191, "getxattr"),
    (192, "lgetxattr"),
    (193, "fgetxattr"),
    (194, "listxattr"),
    (195, "llistxattr"),
    (196, "flistxattr"),
    (197, "removexattr"),
    (198, "lremovexattr"),
    (199, "fremovexattr"),
    (200, "tkill"),
    (201, "time"),
    (202, "futex"),
    (203, "sched_setaffinity"),
    (204, "sched_getaffinity"),
    (205, "set_thread_area"),
    (206, "io_setup"),
    (207, "io_destroy"),
    (208, "io_getevents"),
    (209, "io_submit"),
    (210, "io_cancel"),
    (211, "get_thread_area"),
    (212, "lookup_dcookie"),
    (213, "epoll_create"),
    (214, "epoll_ctl_old"),
    (215, "epoll_wait_old"),
    (216, "remap_file_pages"),
    (217, "getdents64"),
    (218, "set_tid_address"),
    (219, "restart_syscall"),
    (220, "semtimedop"),
    (221, "fadvise64"),
    (222, "timer_create"),
    (223, "timer_settime"),
    (224, "timer_gettime"),
    (225, "timer_getoverrun"),
    (226, "timer_delete"),
    (227, "clock_settime"),
    (228, "clock_gettime"),
    (229, "clock_getres"),
    (230, "clock_nanosleep"),
    (231, "exit_group"),
    (232, "epoll_wait"),
    (233, "epoll_ctl"),
    (234, "tgkill"),
    (235, "utimes"),
    (236, "vserver"),
    (237, "mbind"),
    (238, "set_mempolicy"),
    (239, "get_mempolicy"),
    (240, "mq_open"),
    (241, "mq_unlink"),
    (242, "mq_timedsend"),
    (243, "mq_timedreceive"),
    (244, "mq_notify"),
    (245, "mq_getsetattr"),
    (246, "kexec_load"),
    (247, "waitid"),
    (248, "add_key"),
    (249, "request_key"),
    (250, "keyctl"),
    (251, "ioprio_set"),
    (252, "ioprio_get"),
    (253, "inotify_init"),
    (254, "inotify_add_watch"),
    (255, "inotify_rm_watch"),
    (256, "migrate_pages"),
    (257, "openat"),
    (258, "mkdirat"),
    (259, "mknodat"),
    (260, "fchownat"),
    (261, "futimesat"),
    (262, "newfstatat"),
    (263, "unlinkat"),
    (264, "renameat"),
    (265, "linkat"),
    (266, "symlinkat"),
    (267, "readlinkat"),
    (268, "fchmodat"),
    (269, "faccessat"),
    (270, "pselect6"),
    (271, "ppoll"),
    (272, "unshare"),
    (273, "set_robust_list"),
    (274, "get_robust_list"),
    (275, "splice"),
    (276, "tee"),
    (277, "sync_file_range"),
    (278, "vmsplice"),
    (279, "move_pages"),
    (280, "utimensat"),
    (281, "epoll_pwait"),
    (282, "signalfd"),
    (283, "timerfd_create"),
    (284, "eventfd"),
    (285, "fallocate"),
    (286, "timerfd_settime"),
    (287, "timerfd_gettime"),
    (288, "accept4"),
    (289, "signalfd4"),
    (290, "eventfd2"),
    (291, "epoll_create1"),
    (292, "dup3"),
    (293, "pipe2"),
    (294, "inotify_init1"),
    (295, "preadv"),
    (296, "pwritev"),
    (297, "rt_tgsigqueueinfo"),
    (298, "perf_event_open"),
    (299, "recvmmsg"),
    (300, "fanotify_init"),
    (301, "fanotify_mark"),
    (302, "prlimit64"),
    (303, "name_to_handle_at"),
    (304, "open_by_handle_at"),
    (305, "clock_adjtime"),
    (306, "syncfs"),
    (307, "sendmmsg"),
    (308, "setns"),
    (309, "getcpu"),
    (310, "process_vm_readv"),
    (311, "process_vm_writev"),
    (312, "kcmp"),
    (313, "finit_module"),
    (314, "sched_setattr"),
    (315, "sched_getattr"),
    (316, "renameat2"),
    (317, "seccomp"),
    (318, "getrandom"),
    (319, "memfd_create"),
    (320, "kexec_file_load"),
    (321, "bpf"),
    (322, "execveat"),
    (323, "userfaultfd"),
    (324, "membarrier"),
    (325, "mlock2"),
    (326, "copy_file_range"),
    (327, "preadv2"),
    (328, "pwritev2"),
    (329, "pkey_mprotect"),
    (330, "pkey_alloc"),
    (331, "pkey_free"),
    (332, "statx"),
    (333, "io_pgetevents"),
    (334, "rseq"),
    (335, "uretprobe"),
    (336, "uprobe"),
    (424, "pidfd_send_signal"),
    (425, "io_uring_setup"),
    (426, "io_uring_enter"),
    (427, "io_uring_register"),
    (428, "open_tree"),
    (429, "move_mount"),
    (430, "fsopen"),
    (431, "fsconfig"),
    (432, "fsmount"),
    (433, "fspick"),
    (434, "pidfd_open"),
    (435, "clone3"),
    (436, "close_range"),
    (437, "openat2"),
    (438, "pidfd_getfd"),
    (439, "faccessat2"),
    (440, "process_madvise"),
    (441, "epoll_pwait2"),
    (442, "mount_setattr"),
    (443, "quotactl_fd"),
    (444, "landlock_create_ruleset"),
    (445, "landlock_add_rule"),
    (446, "landlock_restrict_self"),
    (447, "memfd_secret"),
    (448, "process_mrelease"),
    (449, "futex_waitv"),
    (450, "set_mempolicy_home_node"),
    (451, "cachestat"),
    (452, "fchmodat2"),
    (453, "map_shadow_stack"),
    (454, "futex_wake"),
    (455, "futex_wait"),
    (456, "futex_requeue"),
    (457, "statmount"),
    (458, "listmount"),
    (459, "lsm_get_self_attr"),
    (460, "lsm_set_self_attr"),
    (461, "lsm_list_modules"),
    (462, "mseal"),
    (463, "setxattrat"),
    (464, "getxattrat"),
    (465, "listxattrat"),
    (466, "removexattrat"),
    (467, "open_tree_attr"),
    (468, "file_getattr"),
    (469, "file_setattr"),
    (470, "listns"),
    (471, "rseq_slice_yield"),
    (472, "fchroot"),
];

#[cfg(all(any(target_os = "linux", target_os = "android"), target_arch = "aarch64"))]
static SYSCALLS: &[(u32, &str)] = &[
    (0, "io_setup"),
    (1, "io_destroy"),
    (2, "io_submit"),
    (3, "io_cancel"),
    (4, "io_getevents"),
    (5, "setxattr"),
    (6, "lsetxattr"),
    (7, "fsetxattr"),
    (8, "getxattr"),
    (9, "lgetxattr"),
    (10, "fgetxattr"),
    (11, "listxattr"),
    (12, "llistxattr"),
    (13, "flistxattr"),
    (14, "removexattr"),
    (15, "lremovexattr"),
    (16, "fremovexattr"),
    (17, "getcwd"),
    (18, "lookup_dcookie"),
    (19, "eventfd2"),
    (20, "epoll_create1"),
    (21, "epoll_ctl"),
    (22, "epoll_pwait"),
    (23, "dup"),
    (24, "dup3"),
    (25, "fcntl"),
    (26, "inotify_init1"),
    (27, "inotify_add_watch"),
    (28, "inotify_rm_watch"),
    (29, "ioctl"),
    (30, "ioprio_set"),
    (31, "ioprio_get"),
    (32, "flock"),
    (33, "mknodat"),
    (34, "mkdirat"),
    (35, "unlinkat"),
    (36, "symlinkat"),
    (37, "linkat"),
    (39, "umount2"),
    (40, "mount"),
    (41, "pivot_root"),
    (42, "nfsservctl"),
    (43, "statfs"),
    (44, "fstatfs"),
    (45, "truncate"),
    (46, "ftruncate"),
    (47, "fallocate"),
    (48, "faccessat"),
    (49, "chdir"),
    (50, "fchdir"),
    (51, "chroot"),
    (52, "fchmod"),
    (53, "fchmodat"),
    (54, "fchownat"),
    (55, "fchown"),
    (56, "openat"),
    (57, "close"),
    (58, "vhangup"),
    (59, "pipe2"),
    (60, "quotactl"),
    (61, "getdents64"),
    (62, "lseek"),
    (63, "read"),
    (64, "write"),
    (65, "readv"),
    (66, "writev"),
    (67, "pread64"),
    (68, "pwrite64"),
    (69, "preadv"),
    (70, "pwritev"),
    (71, "sendfile"),
    (72, "pselect6"),
    (73, "ppoll"),
    (74, "signalfd4"),
    (75, "vmsplice"),
    (76, "splice"),
    (77, "tee"),
    (78, "readlinkat"),
    (79, "newfstatat"),
    (80, "fstat"),
    (81, "sync"),
    (82, "fsync"),
    (83, "fdatasync"),
    (84, "sync_file_range"),
    (85, "timerfd_create"),
    (86, "timerfd_settime"),
    (87, "timerfd_gettime"),
    (88, "utimensat"),
    (89, "acct"),
    (90, "capget"),
    (91, "capset"),
    (92, "personality"),
    (93, "exit"),
    (94, "exit_group"),
    (95, "waitid"),
    (96, "set_tid_address"),
    (97, "unshare"),
    (98, "futex"),
    (99, "set_robust_list"),
    (100, "get_robust_list"),
    (101, "nanosleep"),
    (102, "getitimer"),
    (103, "setitimer"),
    (104, "kexec_load"),
    (105, "init_module"),
    (106, "delete_module"),
    (107, "timer_create"),
    (108, "timer_gettime"),
    (109, "timer_getoverrun"),
    (110, "timer_settime"),
    (111, "timer_delete"),
    (112, "clock_settime"),
    (113, "clock_gettime"),
    (114, "clock_getres"),
    (115, "clock_nanosleep"),
    (116, "syslog"),
    (117, "ptrace"),
    (118, "sched_setparam"),
    (119, "sched_setscheduler"),
    (120, "sched_getscheduler"),
    (121, "sched_getparam"),
    (122, "sched_setaffinity"),
    (123, "sched_getaffinity"),
    (124, "sched_yield"),
    (125, "sched_get_priority_max"),
    (126, "sched_get_priority_min"),
    (127, "sched_rr_get_interval"),
    (128, "restart_syscall"),
    (129, "kill"),
    (130, "tkill"),
    (131, "tgkill"),
    (132, "sigaltstack"),
    (133, "rt_sigsuspend"),
    (134, "rt_sigaction"),
    (135, "rt_sigprocmask"),
    (136, "rt_sigpending"),
    (137, "rt_sigtimedwait"),
    (138, "rt_sigqueueinfo"),
    (139, "rt_sigreturn"),
    (140, "setpriority"),
    (141, "getpriority"),
    (142, "reboot"),
    (143, "setregid"),
    (144, "setgid"),
    (145, "setreuid"),
    (146, "setuid"),
    (147, "setresuid"),
    (148, "getresuid"),
    (149, "setresgid"),
    (150, "getresgid"),
    (151, "setfsuid"),
    (152, "setfsgid"),
    (153, "times"),
    (154, "setpgid"),
    (155, "getpgid"),
    (156, "getsid"),
    (157, "setsid"),
    (158, "getgroups"),
    (159, "setgroups"),
    (160, "uname"),
    (161, "sethostname"),
    (162, "setdomainname"),
    (165, "getrusage"),
    (166, "umask"),
    (167, "prctl"),
    (168, "getcpu"),
    (169, "gettimeofday"),
    (170, "settimeofday"),
    (171, "adjtimex"),
    (172, "getpid"),
    (173, "getppid"),
    (174, "getuid"),
    (175, "geteuid"),
    (176, "getgid"),
    (177, "getegid"),
    (178, "gettid"),
    (179, "sysinfo"),
    (180, "mq_open"),
    (181, "mq_unlink"),
    (182, "mq_timedsend"),
    (183, "mq_timedreceive"),
    (184, "mq_notify"),
    (185, "mq_getsetattr"),
    (186, "msgget"),
    (187, "msgctl"),
    (188, "msgrcv"),
    (189, "msgsnd"),
    (190, "semget"),
    (191, "semctl"),
    (192, "semtimedop"),
    (193, "semop"),
    (194, "shmget"),
    (195, "shmctl"),
    (196, "shmat"),
    (197, "shmdt"),
    (198, "socket"),
    (199, "socketpair"),
    (200, "bind"),
    (201, "listen"),
    (202, "accept"),
    (203, "connect"),
    (204, "getsockname"),
    (205, "getpeername"),
    (206, "sendto"),
    (207, "recvfrom"),
    (208, "setsockopt"),
    (209, "getsockopt"),
    (210, "shutdown"),
    (211, "sendmsg"),
    (212, "recvmsg"),
    (213, "readahead"),
    (214, "brk"),
    (215, "munmap"),
    (216, "mremap"),
    (217, "add_key"),
    (218, "request_key"),
    (219, "keyctl"),
    (220, "clone"),
    (221, "execve"),
    (222, "mmap"),
    (223, "fadvise64"),
    (224, "swapon"),
    (225, "swapoff"),
    (226, "mprotect"),
    (227, "msync"),
    (228, "mlock"),
    (229, "munlock"),
    (230, "mlockall"),
    (231, "munlockall"),
    (232, "mincore"),
    (233, "madvise"),
    (234, "remap_file_pages"),
    (235, "mbind"),
    (236, "get_mempolicy"),
    (237, "set_mempolicy"),
    (238, "migrate_pages"),
    (239, "move_pages"),
    (240, "rt_tgsigqueueinfo"),
    (241, "perf_event_open"),
    (242, "accept4"),
    (243, "recvmmsg"),
    (260, "wait4"),
    (261, "prlimit64"),
    (262, "fanotify_init"),
    (263, "fanotify_mark"),
    (264, "name_to_handle_at"),
    (265, "open_by_handle_at"),
    (266, "clock_adjtime"),
    (267, "syncfs"),
    (268, "setns"),
    (269, "sendmmsg"),
    (270, "process_vm_readv"),
    (271, "process_vm_writev"),
    (272, "kcmp"),
    (273, "finit_module"),
    (274, "sched_setattr"),
    (275, "sched_getattr"),
    (276, "renameat2"),
    (277, "seccomp"),
    (278, "getrandom"),
    (279, "memfd_create"),
    (280, "bpf"),
    (281, "execveat"),
    (282, "userfaultfd"),
    (283, "membarrier"),
    (284, "mlock2"),
    (285, "copy_file_range"),
    (286, "preadv2"),
    (287, "pwritev2"),
    (288, "pkey_mprotect"),
    (289, "pkey_alloc"),
    (290, "pkey_free"),
    (291, "statx"),
    (292, "io_pgetevents"),
    (293, "rseq"),
    (294, "kexec_file_load"),
    (424, "pidfd_send_signal"),
    (425, "io_uring_setup"),
    (426, "io_uring_enter"),
    (427, "io_uring_register"),
    (428, "open_tree"),
    (429, "move_mount"),
    (430, "fsopen"),
    (431, "fsconfig"),
    (432, "fsmount"),
    (433, "fspick"),
    (434, "pidfd_open"),
    (435, "clone3"),
    (436, "close_range"),
    (437, "openat2"),
    (438, "pidfd_getfd"),
    (439, "faccessat2"),
    (440, "process_madvise"),
    (441, "epoll_pwait2"),
    (442, "mount_setattr"),
    (443, "quotactl_fd"),
    (444, "landlock_create_ruleset"),
    (445, "landlock_add_rule"),
    (446, "landlock_restrict_self"),
    (448, "process_mrelease"),
    (449, "futex_waitv"),
    (450, "set_mempolicy_home_node"),
    (451, "cachestat"),
    (452, "fchmodat2"),
    (453, "map_shadow_stack"),
    (454, "futex_wake"),
    (455, "futex_wait"),
    (456, "futex_requeue"),
    (457, "statmount"),
    (458, "listmount"),
    (459, "lsm_get_self_attr"),
    (460, "lsm_set_self_attr"),
    (461, "lsm_list_modules"),
    (462, "mseal"),
    (463, "setxattrat"),
    (464, "getxattrat"),
    (465, "listxattrat"),
    (466, "removexattrat"),
    (467, "open_tree_attr"),
    (468, "file_getattr"),
    (469, "file_setattr"),
    (470, "listns"),
    (471, "rseq_slice_yield"),
    (472, "fchroot"),
];

/// `نظام.اسم_النداء رقم` — the name of a syscall by its
/// number in this architecture's table.
pub fn syscall_name(num: i64, line: usize, col: usize) -> RioResult<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = num;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        syscall_name_native(num, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn syscall_name_native(num: i64, line: usize, col: usize) -> RioResult<String> {
    #[cfg(all(
        any(target_os = "linux", target_os = "android"),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        if num < 0 || num > u32::MAX as i64 {
            return Err(RioError::runtime(
                format!("نداء غير معروف: {}", num),
                line,
                col,
            ));
        }
        match SYSCALLS.binary_search_by_key(&(num as u32), |&(n, _)| n) {
            Ok(i) => Ok(SYSCALLS[i].1.to_string()),
            Err(_) => Err(RioError::runtime(
                format!("نداء غير معروف: {}", num),
                line,
                col,
            )),
        }
    }
    #[cfg(not(all(
        any(target_os = "linux", target_os = "android"),
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        let _ = num;
        Err(RioError::runtime(
            "نظام.اسم_النداء غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
}

/// `نظام.عدد_النداءات()` — how many syscalls this
/// architecture's native table holds.
pub fn syscall_count(line: usize, col: usize) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        syscall_count_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn syscall_count_native(_line: usize, _col: usize) -> RioResult<u64> {
    #[cfg(all(
        any(target_os = "linux", target_os = "android"),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        Ok(SYSCALLS.len() as u64)
    }
    #[cfg(not(all(
        any(target_os = "linux", target_os = "android"),
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        Err(RioError::runtime(
            "نظام.عدد_النداءات غير مدعوم على هذه المنصة",
            _line,
            _col,
        ))
    }
}

/// One `/proc/sys/kernel/<name>` value, trimmed: the shared
/// reader for the kernel queries on Linux (+Android).
#[cfg(any(target_os = "linux", target_os = "android"))]
fn proc_sys_kernel(name: &str, line: usize, col: usize) -> RioResult<String> {
    let path = format!("/proc/sys/kernel/{}", name);
    let text = std::fs::read_to_string(&path)
        .map_err(|_| RioError::runtime("تعذّر قراءة معلومات النظام", line, col))?;
    Ok(text.trim().to_string())
}

/// One id from `/proc/self/status` (`Uid:` / `Gid:`): the
/// first value on the line is the real id.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn proc_self_id(field: &str, line: usize, col: usize) -> RioResult<u64> {
    let text = std::fs::read_to_string("/proc/self/status")
        .map_err(|_| RioError::runtime("تعذّر قراءة معلومات النظام", line, col))?;
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix(field) {
            if let Some(first) = rest.split_whitespace().next() {
                if let Ok(id) = first.parse::<u64>() {
                    return Ok(id);
                }
            }
            break;
        }
    }
    Err(RioError::runtime("تعذّر قراءة معلومات النظام", line, col))
}

/// `نظام.نسخة_النواة()` — the running kernel release.
pub fn kernel_version(line: usize, col: usize) -> RioResult<String> {
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        kernel_version_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn kernel_version_native(line: usize, col: usize) -> RioResult<String> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        proc_sys_kernel("osrelease", line, col)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        Err(RioError::runtime(
            "نظام.نسخة_النواة غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
}

/// `نظام.اسم_النظام()` — the OS name from the kernel.
pub fn os_name(line: usize, col: usize) -> RioResult<String> {
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        os_name_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn os_name_native(line: usize, col: usize) -> RioResult<String> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        proc_sys_kernel("ostype", line, col)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        Err(RioError::runtime(
            "نظام.اسم_النظام غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
}

/// `نظام.معرف_المستخدم()` — the real user id.
pub fn user_id(line: usize, col: usize) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        user_id_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn user_id_native(line: usize, col: usize) -> RioResult<u64> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        proc_self_id("Uid:", line, col)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        Err(RioError::runtime(
            "نظام.معرف_المستخدم غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
}

/// `نظام.معرف_المجموعة()` — the real group id.
pub fn group_id(line: usize, col: usize) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        group_id_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn group_id_native(line: usize, col: usize) -> RioResult<u64> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        proc_self_id("Gid:", line, col)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        Err(RioError::runtime(
            "نظام.معرف_المجموعة غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_sorted_no_duplicates() {
        // The table is sorted by number with no repeats, so
        // the binary search is sound on every target that
        // has a table at all.
        #[cfg(all(
            any(target_os = "linux", target_os = "android"),
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        {
            for w in SYSCALLS.windows(2) {
                assert!(w[0].0 < w[1].0, "table not sorted: {:?}", w);
            }
            assert!(!SYSCALLS.is_empty(), "table must not be empty");
        }
    }

    #[test]
    fn table_count_is_native() {
        // x86-64: 386 native entries (339 common + 47
        // 64-bit-only). L1's 422 adds the 36 x32 compat
        // entries, which are not native 64-bit syscalls.
        // aarch64: 324 native entries (287 common + 37
        // 64-bit-only).
        #[cfg(all(
            any(target_os = "linux", target_os = "android"),
            target_arch = "x86_64"
        ))]
        {
            assert_eq!(SYSCALLS.len(), 386);
            assert_eq!(syscall_count(1, 1).unwrap(), 386);
        }
        #[cfg(all(
            any(target_os = "linux", target_os = "android"),
            target_arch = "aarch64"
        ))]
        {
            assert_eq!(SYSCALLS.len(), 324);
            assert_eq!(syscall_count(1, 1).unwrap(), 324);
        }
    }

    #[test]
    fn name_known_x86_64() {
        #[cfg(all(
            any(target_os = "linux", target_os = "android"),
            target_arch = "x86_64"
        ))]
        {
            assert_eq!(syscall_name(0, 1, 1).unwrap(), "read");
            assert_eq!(syscall_name(1, 1, 1).unwrap(), "write");
            assert_eq!(syscall_name(59, 1, 1).unwrap(), "execve");
            assert_eq!(syscall_name(60, 1, 1).unwrap(), "exit");
            assert_eq!(syscall_name(231, 1, 1).unwrap(), "exit_group");
            assert_eq!(syscall_name(472, 1, 1).unwrap(), "fchroot");
        }
    }

    #[test]
    fn name_known_aarch64() {
        #[cfg(all(
            any(target_os = "linux", target_os = "android"),
            target_arch = "aarch64"
        ))]
        {
            assert_eq!(syscall_name(0, 1, 1).unwrap(), "io_setup");
            assert_eq!(syscall_name(63, 1, 1).unwrap(), "read");
            assert_eq!(syscall_name(64, 1, 1).unwrap(), "write");
            assert_eq!(syscall_name(93, 1, 1).unwrap(), "exit");
            assert_eq!(syscall_name(472, 1, 1).unwrap(), "fchroot");
        }
    }

    #[test]
    fn name_unknown_is_arabic() {
        // A gap in the table, a negative number, and a
        // number past the end: all refuse in Arabic.
        // (The gap is a real hole in the x86-64 table on
        // this host; on aarch64 the same check runs against
        // its own table — 99999 is past the end on both.)
        #[cfg(all(
            any(target_os = "linux", target_os = "android"),
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        {
            for bad in [-1, 99999] {
                let err = syscall_name(bad, 1, 1).unwrap_err().to_string();
                assert!(err.contains("نداء غير معروف"), "got: {}", err);
            }
        }
        #[cfg(all(
            any(target_os = "linux", target_os = "android"),
            target_arch = "x86_64"
        ))]
        {
            let err = syscall_name(337, 1, 1).unwrap_err().to_string();
            assert!(err.contains("نداء غير معروف"), "got: {}", err);
        }
    }

    #[test]
    fn kernel_queries_match_proc() {
        // The values must match the system, not just be
        // non-empty: compare with a direct /proc read.
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let expected = std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .unwrap()
                .trim()
                .to_string();
            assert!(!expected.is_empty(), "osrelease must not be empty");
            assert_eq!(kernel_version(1, 1).unwrap(), expected);
            assert_eq!(os_name(1, 1).unwrap(), "Linux");
        }
    }

    #[test]
    fn ids_match_status() {
        // Uid/Gid must match /proc/self/status exactly.
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let text = std::fs::read_to_string("/proc/self/status").unwrap();
            let mut uid = 0u64;
            let mut gid = 0u64;
            for l in text.lines() {
                if let Some(rest) = l.strip_prefix("Uid:") {
                    uid = rest
                        .split_whitespace()
                        .next()
                        .unwrap()
                        .parse()
                        .unwrap();
                }
                if let Some(rest) = l.strip_prefix("Gid:") {
                    gid = rest
                        .split_whitespace()
                        .next()
                        .unwrap()
                        .parse()
                        .unwrap();
                }
            }
            assert_eq!(user_id(1, 1).unwrap(), uid);
            assert_eq!(group_id(1, 1).unwrap(), gid);
        }
    }

    #[test]
    fn queries_refused_elsewhere() {
        // macOS / Windows: an honest Arabic refusal, never
        // an invented value and never a crash.
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            for err in [
                syscall_name(0, 1, 1).unwrap_err().to_string(),
                syscall_count(1, 1).unwrap_err().to_string(),
                kernel_version(1, 1).unwrap_err().to_string(),
                os_name(1, 1).unwrap_err().to_string(),
                user_id(1, 1).unwrap_err().to_string(),
                group_id(1, 1).unwrap_err().to_string(),
            ] {
                assert!(err.contains("غير مدعوم"), "got: {}", err);
            }
        }
    }
}
