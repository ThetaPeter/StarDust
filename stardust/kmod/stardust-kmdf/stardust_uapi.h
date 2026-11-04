/* StarDust Windows KMDF UAPI. Do not include the Linux header (linux/ioctl.h).
 * CREATE fields match kmod/stardust/stardust_uapi.h (name[64] incl. NUL, u64 size, u64 align).
 * IOCTL numbers are Windows CTL_CODE values, not Linux _IOW.
 */
#ifndef STARDUST_UAPI_H
#define STARDUST_UAPI_H

#define STARDUST_NAME_LEN 64

#if defined(_MSC_VER)
#pragma pack(push, 1)
#endif
struct stardust_create_arg {
	char name[STARDUST_NAME_LEN];
	unsigned long long size;
	unsigned long long align;
};

struct stardust_map_out {
	unsigned long long user_ptr;
	unsigned long long size;
};
#if defined(_MSC_VER)
#pragma pack(pop)
#endif

#define STARDUST_IOCTL_CREATE \
	CTL_CODE(FILE_DEVICE_UNKNOWN, 0x800, METHOD_BUFFERED, FILE_ANY_ACCESS)
#define STARDUST_IOCTL_MAP \
	CTL_CODE(FILE_DEVICE_UNKNOWN, 0x801, METHOD_BUFFERED, FILE_ANY_ACCESS)
#define STARDUST_IOCTL_ATTACH \
	CTL_CODE(FILE_DEVICE_UNKNOWN, 0x802, METHOD_BUFFERED, FILE_ANY_ACCESS)

#endif /* STARDUST_UAPI_H */
