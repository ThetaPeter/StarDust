/* SPDX-License-Identifier: GPL-2.0 */
#ifndef STARDUST_UAPI_H
#define STARDUST_UAPI_H

#include <linux/ioctl.h>
#include <linux/types.h>

/* Unique region name, including the terminating NUL. */
#define STARDUST_NAME_LEN 64

struct stardust_create_arg {
	char name[STARDUST_NAME_LEN];
	__u64 size;
	__u64 align;
};

#define STARDUST_IOC_MAGIC 'S'
#define STARDUST_IOC_CREATE \
	_IOW(STARDUST_IOC_MAGIC, 1, struct stardust_create_arg)
#define STARDUST_IOC_ATTACH \
	_IOWR(STARDUST_IOC_MAGIC, 2, struct stardust_create_arg)

#endif /* STARDUST_UAPI_H */
