/*
 * StarDust named region backing (KMDF): CREATE / ATTACH + MAP on \\.\stardust.
 * Refcounted last close across all handles frees the name and pages.
 * ATTACH does not map; MAP on that handle maps this process. No DESTROY, layout, or FFI.
 */

#include <ntifs.h>
#include <wdf.h>
#include <wdmsec.h>

#include "stardust_uapi.h"

#define STARDUST_POOL_TAG 'dStS'

static const WCHAR StardustSddl[] =
	L"D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;WD)(A;;GRGW;;;RC)";

typedef struct _STARDUST_REGION {
	LIST_ENTRY List;
	CHAR Name[STARDUST_NAME_LEN];
	SIZE_T MappedSize;
	ULONGLONG UserSize;
	ULONGLONG UserAlign;
	ULONG RefCount;
	HANDLE SectionHandle;
} STARDUST_REGION, *PSTARDUST_REGION;

typedef struct _STARDUST_DEVICE_CONTEXT {
	WDFWAITLOCK Lock;
	LIST_ENTRY Names;
} STARDUST_DEVICE_CONTEXT, *PSTARDUST_DEVICE_CONTEXT;

WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(STARDUST_DEVICE_CONTEXT, DeviceGetContext);

typedef struct _STARDUST_FILE_CONTEXT {
	PSTARDUST_REGION Region;
	BOOLEAN Mapped;
	PVOID UserVa;
	PEPROCESS Process;
} STARDUST_FILE_CONTEXT, *PSTARDUST_FILE_CONTEXT;

WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(STARDUST_FILE_CONTEXT, FileGetContext);

DRIVER_INITIALIZE DriverEntry;
EVT_WDF_DRIVER_DEVICE_ADD StardustEvtDeviceAdd;
EVT_WDF_IO_QUEUE_IO_DEVICE_CONTROL StardustEvtIoDeviceControl;
EVT_WDF_FILE_CLEANUP StardustEvtFileCleanup;

static BOOLEAN
NameValid(_In_reads_(STARDUST_NAME_LEN) const CHAR *Name)
{
	ULONG i;

	if (Name[0] == '\0') {
		return FALSE;
	}
	for (i = 0; i < STARDUST_NAME_LEN; i++) {
		if (Name[i] == '\0') {
			return TRUE;
		}
	}
	return FALSE;
}

static BOOLEAN
AlignIsPow2(unsigned long long Align)
{
	return Align != 0 && (Align & (Align - 1)) == 0;
}

static PSTARDUST_REGION
FindByName(_In_ PSTARDUST_DEVICE_CONTEXT DevCtx, _In_reads_(STARDUST_NAME_LEN) const CHAR *Name)
{
	PLIST_ENTRY entry;

	for (entry = DevCtx->Names.Flink; entry != &DevCtx->Names; entry = entry->Flink) {
		PSTARDUST_REGION r = CONTAINING_RECORD(entry, STARDUST_REGION, List);
		if (RtlEqualMemory(r->Name, Name, STARDUST_NAME_LEN)) {
			return r;
		}
	}
	return NULL;
}

static VOID
FreeRegionLocked(_Inout_ PSTARDUST_REGION Region)
{
	if (Region == NULL) {
		return;
	}
	RemoveEntryList(&Region->List);
	if (Region->SectionHandle != NULL) {
		ZwClose(Region->SectionHandle);
		Region->SectionHandle = NULL;
	}
	ExFreePoolWithTag(Region, STARDUST_POOL_TAG);
}

static VOID
UnmapIfNeededLocked(_Inout_ PSTARDUST_FILE_CONTEXT FileCtx)
{
	KAPC_STATE apc;

	if (!FileCtx->Mapped || FileCtx->UserVa == NULL) {
		return;
	}
	if (FileCtx->Process != NULL) {
		KeStackAttachProcess(FileCtx->Process, &apc);
		(VOID)ZwUnmapViewOfSection(ZwCurrentProcess(), FileCtx->UserVa);
		KeUnstackDetachProcess(&apc);
		ObDereferenceObject(FileCtx->Process);
		FileCtx->Process = NULL;
	}
	FileCtx->Mapped = FALSE;
	FileCtx->UserVa = NULL;
}

static NTSTATUS
IoctlCreate(_In_ WDFREQUEST Request, _In_ PSTARDUST_DEVICE_CONTEXT DevCtx, _In_ PSTARDUST_FILE_CONTEXT FileCtx)
{
	NTSTATUS status;
	struct stardust_create_arg *req;
	size_t inLen = 0;
	PSTARDUST_REGION reg;
	SIZE_T mapped;
	OBJECT_ATTRIBUTES oa;
	LARGE_INTEGER maxSize;
	HANDLE section = NULL;

	status = WdfRequestRetrieveInputBuffer(Request, sizeof(*req), (PVOID *)&req, &inLen);
	if (!NT_SUCCESS(status)) {
		return status;
	}

	if (!NameValid(req->name) || req->size == 0 || !AlignIsPow2(req->align)) {
		return STATUS_INVALID_PARAMETER;
	}
	if (req->size > (unsigned long long)(MAXULONG_PTR - PAGE_SIZE)) {
		return STATUS_INVALID_PARAMETER;
	}

	mapped = ROUND_TO_PAGES((SIZE_T)req->size);
	if (mapped == 0) {
		return STATUS_INVALID_PARAMETER;
	}

	WdfWaitLockAcquire(DevCtx->Lock, NULL);
	if (FileCtx->Region != NULL) {
		WdfWaitLockRelease(DevCtx->Lock);
		return STATUS_INVALID_DEVICE_REQUEST;
	}
	if (FindByName(DevCtx, req->name) != NULL) {
		WdfWaitLockRelease(DevCtx->Lock);
		return STATUS_OBJECT_NAME_COLLISION;
	}

	reg = ExAllocatePoolWithTag(PagedPool, sizeof(*reg), STARDUST_POOL_TAG);
	if (reg == NULL) {
		WdfWaitLockRelease(DevCtx->Lock);
		return STATUS_INSUFFICIENT_RESOURCES;
	}
	RtlZeroMemory(reg, sizeof(*reg));
	RtlCopyMemory(reg->Name, req->name, STARDUST_NAME_LEN);
	reg->MappedSize = mapped;
	reg->UserSize = req->size;
	reg->UserAlign = req->align;
	reg->RefCount = 1;

	maxSize.QuadPart = (LONGLONG)mapped;
	InitializeObjectAttributes(&oa, NULL, OBJ_KERNEL_HANDLE, NULL, NULL);
	status = ZwCreateSection(
		&section,
		SECTION_MAP_READ | SECTION_MAP_WRITE,
		&oa,
		&maxSize,
		PAGE_READWRITE,
		SEC_COMMIT,
		NULL);
	if (!NT_SUCCESS(status)) {
		ExFreePoolWithTag(reg, STARDUST_POOL_TAG);
		WdfWaitLockRelease(DevCtx->Lock);
		return status;
	}

	reg->SectionHandle = section;
	InsertHeadList(&DevCtx->Names, &reg->List);
	FileCtx->Region = reg;
	WdfWaitLockRelease(DevCtx->Lock);
	return STATUS_SUCCESS;
}

static NTSTATUS
IoctlMap(_In_ WDFREQUEST Request, _In_ PSTARDUST_DEVICE_CONTEXT DevCtx, _In_ PSTARDUST_FILE_CONTEXT FileCtx)
{
	NTSTATUS status;
	struct stardust_map_out *out;
	size_t outLen = 0;
	PVOID userVa;
	SIZE_T viewSize;
	PEPROCESS process;

	status = WdfRequestRetrieveOutputBuffer(Request, sizeof(*out), (PVOID *)&out, &outLen);
	if (!NT_SUCCESS(status)) {
		return status;
	}

	WdfWaitLockAcquire(DevCtx->Lock, NULL);
	if (FileCtx->Region == NULL || FileCtx->Mapped) {
		WdfWaitLockRelease(DevCtx->Lock);
		return STATUS_INVALID_DEVICE_REQUEST;
	}

	userVa = NULL;
	viewSize = FileCtx->Region->MappedSize;
	status = ZwMapViewOfSection(
		FileCtx->Region->SectionHandle,
		ZwCurrentProcess(),
		&userVa,
		0,
		0,
		NULL,
		&viewSize,
		ViewUnmap,
		0,
		PAGE_READWRITE);
	if (!NT_SUCCESS(status) || userVa == NULL) {
		WdfWaitLockRelease(DevCtx->Lock);
		if (NT_SUCCESS(status)) {
			status = STATUS_INSUFFICIENT_RESOURCES;
		}
		return status;
	}

	process = PsGetCurrentProcess();
	ObReferenceObject(process);
	FileCtx->Process = process;
	FileCtx->UserVa = userVa;
	FileCtx->Mapped = TRUE;

	out->user_ptr = (unsigned long long)(ULONG_PTR)userVa;
	out->size = (unsigned long long)viewSize;
	WdfWaitLockRelease(DevCtx->Lock);
	return STATUS_SUCCESS;
}

static NTSTATUS
IoctlAttach(_In_ WDFREQUEST Request, _In_ PSTARDUST_DEVICE_CONTEXT DevCtx, _In_ PSTARDUST_FILE_CONTEXT FileCtx)
{
	NTSTATUS status;
	struct stardust_create_arg *req;
	struct stardust_create_arg *out;
	size_t inLen = 0;
	size_t outLen = 0;
	PSTARDUST_REGION reg;

	status = WdfRequestRetrieveInputBuffer(Request, sizeof(*req), (PVOID *)&req, &inLen);
	if (!NT_SUCCESS(status)) {
		return status;
	}
	status = WdfRequestRetrieveOutputBuffer(Request, sizeof(*out), (PVOID *)&out, &outLen);
	if (!NT_SUCCESS(status)) {
		return status;
	}

	if (!NameValid(req->name)) {
		return STATUS_INVALID_PARAMETER;
	}

	WdfWaitLockAcquire(DevCtx->Lock, NULL);
	if (FileCtx->Region != NULL) {
		WdfWaitLockRelease(DevCtx->Lock);
		return STATUS_INVALID_DEVICE_REQUEST;
	}
	reg = FindByName(DevCtx, req->name);
	if (reg == NULL) {
		WdfWaitLockRelease(DevCtx->Lock);
		return STATUS_OBJECT_NAME_NOT_FOUND;
	}

	FileCtx->Region = reg;
	reg->RefCount++;
	RtlCopyMemory(out->name, reg->Name, STARDUST_NAME_LEN);
	out->size = reg->UserSize;
	out->align = reg->UserAlign;
	WdfWaitLockRelease(DevCtx->Lock);
	return STATUS_SUCCESS;
}

VOID
StardustEvtFileCleanup(_In_ WDFFILEOBJECT FileObject)
{
	PSTARDUST_FILE_CONTEXT fileCtx = FileGetContext(FileObject);
	WDFDEVICE device = WdfFileObjectGetDevice(FileObject);
	PSTARDUST_DEVICE_CONTEXT devCtx = DeviceGetContext(device);

	WdfWaitLockAcquire(devCtx->Lock, NULL);
	UnmapIfNeededLocked(fileCtx);
	if (fileCtx->Region != NULL) {
		if (fileCtx->Region->RefCount > 0) {
			fileCtx->Region->RefCount--;
		}
		if (fileCtx->Region->RefCount == 0) {
			FreeRegionLocked(fileCtx->Region);
		}
		fileCtx->Region = NULL;
	}
	WdfWaitLockRelease(devCtx->Lock);
}

VOID
StardustEvtIoDeviceControl(
	_In_ WDFQUEUE Queue,
	_In_ WDFREQUEST Request,
	_In_ size_t OutputBufferLength,
	_In_ size_t InputBufferLength,
	_In_ ULONG IoControlCode)
{
	WDFDEVICE device = WdfIoQueueGetDevice(Queue);
	PSTARDUST_DEVICE_CONTEXT devCtx = DeviceGetContext(device);
	WDFFILEOBJECT fileObject = WdfRequestGetFileObject(Request);
	PSTARDUST_FILE_CONTEXT fileCtx;
	NTSTATUS status;
	ULONG_PTR info = 0;

	UNREFERENCED_PARAMETER(OutputBufferLength);
	UNREFERENCED_PARAMETER(InputBufferLength);

	if (fileObject == NULL) {
		WdfRequestComplete(Request, STATUS_INVALID_HANDLE);
		return;
	}
	fileCtx = FileGetContext(fileObject);

	switch (IoControlCode) {
	case STARDUST_IOCTL_CREATE:
		status = IoctlCreate(Request, devCtx, fileCtx);
		break;
	case STARDUST_IOCTL_ATTACH:
		status = IoctlAttach(Request, devCtx, fileCtx);
		if (NT_SUCCESS(status)) {
			info = sizeof(struct stardust_create_arg);
		}
		break;
	case STARDUST_IOCTL_MAP:
		status = IoctlMap(Request, devCtx, fileCtx);
		if (NT_SUCCESS(status)) {
			info = sizeof(struct stardust_map_out);
		}
		break;
	default:
		status = STATUS_INVALID_DEVICE_REQUEST;
		break;
	}

	WdfRequestCompleteWithInformation(Request, status, info);
}

NTSTATUS
StardustEvtDeviceAdd(_In_ WDFDRIVER Driver, _Inout_ PWDFDEVICE_INIT DeviceInit)
{
	NTSTATUS status;
	WDFDEVICE device;
	WDF_OBJECT_ATTRIBUTES devAttr;
	WDF_OBJECT_ATTRIBUTES fileAttr;
	WDF_OBJECT_ATTRIBUTES lockAttr;
	WDF_FILEOBJECT_CONFIG fileConfig;
	WDF_IO_QUEUE_CONFIG queueConfig;
	WDFQUEUE queue;
	PSTARDUST_DEVICE_CONTEXT devCtx;
	UNICODE_STRING sddl;
	DECLARE_CONST_UNICODE_STRING(symlink, L"\\DosDevices\\stardust");

	UNREFERENCED_PARAMETER(Driver);

	WdfDeviceInitSetDeviceType(DeviceInit, FILE_DEVICE_UNKNOWN);
	WdfDeviceInitSetIoType(DeviceInit, WdfDeviceIoBuffered);
	WdfDeviceInitSetExclusive(DeviceInit, FALSE);

	RtlInitUnicodeString(&sddl, StardustSddl);
	status = WdfDeviceInitAssignSDDLString(DeviceInit, &sddl);
	if (!NT_SUCCESS(status)) {
		return status;
	}

	WDF_FILEOBJECT_CONFIG_INIT(&fileConfig, WDF_NO_EVENT_CALLBACK, WDF_NO_EVENT_CALLBACK, StardustEvtFileCleanup);
	WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&fileAttr, STARDUST_FILE_CONTEXT);
	WdfDeviceInitSetFileObjectConfig(DeviceInit, &fileConfig, &fileAttr);

	WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&devAttr, STARDUST_DEVICE_CONTEXT);
	devAttr.ExecutionLevel = WdfExecutionLevelPassive;
	status = WdfDeviceCreate(&DeviceInit, &devAttr, &device);
	if (!NT_SUCCESS(status)) {
		return status;
	}

	devCtx = DeviceGetContext(device);
	InitializeListHead(&devCtx->Names);

	WDF_OBJECT_ATTRIBUTES_INIT(&lockAttr);
	lockAttr.ParentObject = device;
	status = WdfWaitLockCreate(&lockAttr, &devCtx->Lock);
	if (!NT_SUCCESS(status)) {
		return status;
	}

	status = WdfDeviceCreateSymbolicLink(device, &symlink);
	if (!NT_SUCCESS(status)) {
		return status;
	}

	WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(&queueConfig, WdfIoQueueDispatchParallel);
	queueConfig.EvtIoDeviceControl = StardustEvtIoDeviceControl;
	queueConfig.PowerManaged = WdfFalse;
	status = WdfIoQueueCreate(device, &queueConfig, WDF_NO_OBJECT_ATTRIBUTES, &queue);
	if (!NT_SUCCESS(status)) {
		return status;
	}

	return STATUS_SUCCESS;
}

NTSTATUS
DriverEntry(_In_ PDRIVER_OBJECT DriverObject, _In_ PUNICODE_STRING RegistryPath)
{
	WDF_DRIVER_CONFIG config;

	WDF_DRIVER_CONFIG_INIT(&config, StardustEvtDeviceAdd);
	return WdfDriverCreate(DriverObject, RegistryPath, WDF_NO_OBJECT_ATTRIBUTES, &config, WDF_NO_HANDLE);
}
