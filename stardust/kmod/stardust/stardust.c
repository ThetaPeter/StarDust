// SPDX-License-Identifier: GPL-2.0
/*
 * StarDust named region backing: CREATE/ATTACH ioctl + mmap on /dev/stardust.
 * Refcounted last close across all fds frees the name and pages.
 * No DESTROY, no layout or FFI in this module.
 */

#include <linux/fs.h>
#include <linux/list.h>
#include <linux/miscdevice.h>
#include <linux/mm.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/slab.h>
#include <linux/string.h>
#include <linux/uaccess.h>
#include <linux/vmalloc.h>

#include "stardust_uapi.h"

struct stardust_region {
	char name[STARDUST_NAME_LEN];
	void *buf;
	size_t mapped_size;
	u64 user_size;
	u64 user_align;
	int refcount;
	struct list_head list;
};

struct stardust_file {
	struct stardust_region *region;
};

static LIST_HEAD(stardust_names);
static DEFINE_MUTEX(stardust_lock);

static bool name_valid(const char *name)
{
	int i;

	if (name[0] == '\0')
		return false;
	for (i = 0; i < STARDUST_NAME_LEN; i++) {
		if (name[i] == '\0')
			return true;
	}
	return false;
}

static bool align_is_pow2(u64 align)
{
	return align != 0 && (align & (align - 1)) == 0;
}

static struct stardust_region *find_by_name(const char *name)
{
	struct stardust_region *r;

	list_for_each_entry(r, &stardust_names, list) {
		if (strncmp(r->name, name, STARDUST_NAME_LEN) == 0)
			return r;
	}
	return NULL;
}

static void free_region(struct stardust_region *r)
{
	if (!r)
		return;
	list_del(&r->list);
	vfree(r->buf);
	kfree(r);
}

static int stardust_open(struct inode *inode, struct file *filp)
{
	struct stardust_file *priv;

	priv = kzalloc(sizeof(*priv), GFP_KERNEL);
	if (!priv)
		return -ENOMEM;
	filp->private_data = priv;
	return 0;
}

static int stardust_release(struct inode *inode, struct file *filp)
{
	struct stardust_file *priv = filp->private_data;

	mutex_lock(&stardust_lock);
	if (priv->region) {
		priv->region->refcount--;
		if (priv->region->refcount == 0)
			free_region(priv->region);
		priv->region = NULL;
	}
	mutex_unlock(&stardust_lock);
	kfree(priv);
	return 0;
}

static long ioctl_create(struct stardust_file *priv, unsigned long arg)
{
	struct stardust_create_arg req;
	struct stardust_region *reg;
	unsigned long mapped;

	if (copy_from_user(&req, (void __user *)arg, sizeof(req)))
		return -EFAULT;

	if (!name_valid(req.name) || !req.size || !align_is_pow2(req.align))
		return -EINVAL;
	if (req.size > (u64)(SIZE_MAX - PAGE_SIZE))
		return -EINVAL;

	mapped = PAGE_ALIGN((unsigned long)req.size);
	if (!mapped)
		return -EINVAL;

	mutex_lock(&stardust_lock);
	if (priv->region) {
		mutex_unlock(&stardust_lock);
		return -EINVAL;
	}
	if (find_by_name(req.name)) {
		mutex_unlock(&stardust_lock);
		return -EEXIST;
	}

	reg = kzalloc(sizeof(*reg), GFP_KERNEL);
	if (!reg) {
		mutex_unlock(&stardust_lock);
		return -ENOMEM;
	}
	memcpy(reg->name, req.name, STARDUST_NAME_LEN);
	reg->mapped_size = mapped;
	reg->user_size = req.size;
	reg->user_align = req.align;
	reg->refcount = 1;
	reg->buf = vmalloc_user(mapped);
	if (!reg->buf) {
		kfree(reg);
		mutex_unlock(&stardust_lock);
		return -ENOMEM;
	}
	list_add(&reg->list, &stardust_names);
	priv->region = reg;
	mutex_unlock(&stardust_lock);
	return 0;
}

static long ioctl_attach(struct stardust_file *priv, unsigned long arg)
{
	struct stardust_create_arg req;
	struct stardust_region *reg;

	if (copy_from_user(&req, (void __user *)arg, sizeof(req)))
		return -EFAULT;

	if (!name_valid(req.name))
		return -EINVAL;

	mutex_lock(&stardust_lock);
	if (priv->region) {
		mutex_unlock(&stardust_lock);
		return -EINVAL;
	}
	reg = find_by_name(req.name);
	if (!reg) {
		mutex_unlock(&stardust_lock);
		return -ENOENT;
	}
	priv->region = reg;
	reg->refcount++;
	req.size = reg->user_size;
	req.align = reg->user_align;
	mutex_unlock(&stardust_lock);

	if (copy_to_user((void __user *)arg, &req, sizeof(req))) {
		mutex_lock(&stardust_lock);
		if (priv->region == reg) {
			reg->refcount--;
			priv->region = NULL;
			if (reg->refcount == 0)
				free_region(reg);
		}
		mutex_unlock(&stardust_lock);
		return -EFAULT;
	}
	return 0;
}

static long stardust_ioctl(struct file *filp, unsigned int cmd, unsigned long arg)
{
	struct stardust_file *priv = filp->private_data;

	switch (cmd) {
	case STARDUST_IOC_CREATE:
		return ioctl_create(priv, arg);
	case STARDUST_IOC_ATTACH:
		return ioctl_attach(priv, arg);
	default:
		return -ENOTTY;
	}
}

static int stardust_mmap(struct file *filp, struct vm_area_struct *vma)
{
	struct stardust_file *priv = filp->private_data;
	unsigned long len = vma->vm_end - vma->vm_start;
	int ret;

	mutex_lock(&stardust_lock);
	if (!priv->region || vma->vm_pgoff != 0 || len > priv->region->mapped_size) {
		mutex_unlock(&stardust_lock);
		return -EINVAL;
	}
	ret = remap_vmalloc_range(vma, priv->region->buf, 0);
	mutex_unlock(&stardust_lock);
	return ret;
}

static const struct file_operations stardust_fops = {
	.owner = THIS_MODULE,
	.open = stardust_open,
	.release = stardust_release,
	.unlocked_ioctl = stardust_ioctl,
	.mmap = stardust_mmap,
	.llseek = noop_llseek,
};

static struct miscdevice stardust_misc = {
	.minor = MISC_DYNAMIC_MINOR,
	.name = "stardust",
	.fops = &stardust_fops,
	.mode = 0666,
};

static int __init stardust_init(void)
{
	return misc_register(&stardust_misc);
}

static void __exit stardust_exit(void)
{
	misc_deregister(&stardust_misc);
}

module_init(stardust_init);
module_exit(stardust_exit);

MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("StarDust named region backing");
MODULE_AUTHOR("StarDust");
