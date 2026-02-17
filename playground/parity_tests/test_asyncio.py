# HOST MODULE: Only testing sandbox-safe subset
# Testing asyncio module API existence without running async code

import asyncio

# === Future class ===
try:
    print('future_class_exists', asyncio.Future is not None)
    print('future_class_is_class', isinstance(asyncio.Future, type))
except Exception as e:
    print('SKIP_Future_class', type(e).__name__, e)

# === Task class ===
try:
    print('task_class_exists', asyncio.Task is not None)
    print('task_class_is_class', isinstance(asyncio.Task, type))
except Exception as e:
    print('SKIP_Task_class', type(e).__name__, e)

# === Queue classes ===
try:
    print('queue_class_exists', asyncio.Queue is not None)
    print('queue_class_is_class', isinstance(asyncio.Queue, type))
    print('lifoqueue_class_exists', asyncio.LifoQueue is not None)
    print('lifoqueue_class_is_class', isinstance(asyncio.LifoQueue, type))
    print('priorityqueue_class_exists', asyncio.PriorityQueue is not None)
    print('priorityqueue_class_is_class', isinstance(asyncio.PriorityQueue, type))
    print('queueempty_exists', asyncio.QueueEmpty is not None)
    print('queuefull_exists', asyncio.QueueFull is not None)
    print('queueshutdown_exists', asyncio.QueueShutDown is not None)
except Exception as e:
    print('SKIP_Queue_classes', type(e).__name__, e)

# === Synchronization primitives ===
try:
    print('lock_class_exists', asyncio.Lock is not None)
    print('lock_class_is_class', isinstance(asyncio.Lock, type))
    print('event_class_exists', asyncio.Event is not None)
    print('event_class_is_class', isinstance(asyncio.Event, type))
    print('semaphore_class_exists', asyncio.Semaphore is not None)
    print('semaphore_class_is_class', isinstance(asyncio.Semaphore, type))
    print('boundedsemaphore_class_exists', asyncio.BoundedSemaphore is not None)
    print('boundedsemaphore_class_is_class', isinstance(asyncio.BoundedSemaphore, type))
    print('condition_class_exists', asyncio.Condition is not None)
    print('condition_class_is_class', isinstance(asyncio.Condition, type))
    print('barrier_class_exists', asyncio.Barrier is not None)
    print('barrier_class_is_class', isinstance(asyncio.Barrier, type))
except Exception as e:
    print('SKIP_Synchronization_primitives', type(e).__name__, e)

# === Exception classes ===
try:
    print('cancellederror_exists', asyncio.CancelledError is not None)
    print('timeouterror_exists', asyncio.TimeoutError is not None)
    print('invalidstateerror_exists', asyncio.InvalidStateError is not None)
    print('incompletereaderror_exists', asyncio.IncompleteReadError is not None)
    print('limitoverrunerror_exists', asyncio.LimitOverrunError is not None)
    print('brokenbarriererror_exists', asyncio.BrokenBarrierError is not None)
except Exception as e:
    print('SKIP_Exception_classes', type(e).__name__, e)

# === Utility functions (non-async) ===
try:
    print('gather_function_exists', callable(asyncio.gather))
    print('wait_function_exists', callable(asyncio.wait))
    print('wait_for_function_exists', callable(asyncio.wait_for))
    print('sleep_function_exists', callable(asyncio.sleep))
    print('shield_function_exists', callable(asyncio.shield))
except Exception as e:
    print('SKIP_Utility_functions_non_async', type(e).__name__, e)

# === Type checking functions ===
try:
    print('iscoroutine_function_exists', callable(asyncio.iscoroutine))
    print('iscoroutinefunction_function_exists', callable(asyncio.iscoroutinefunction))
    print('isfuture_function_exists', callable(asyncio.isfuture))
except Exception as e:
    print('SKIP_Type_checking_functions', type(e).__name__, e)

# === Constants ===
try:
    print('all_completed_constant_exists', hasattr(asyncio, 'ALL_COMPLETED'))
    print('first_completed_constant_exists', hasattr(asyncio, 'FIRST_COMPLETED'))
    print('first_exception_constant_exists', hasattr(asyncio, 'FIRST_EXCEPTION'))
except Exception as e:
    print('SKIP_Constants', type(e).__name__, e)

# === Task functions ===
try:
    print('create_task_function_exists', callable(asyncio.create_task))
    print('current_task_function_exists', callable(asyncio.current_task))
    print('all_tasks_function_exists', callable(asyncio.all_tasks))
    print('ensure_future_function_exists', callable(asyncio.ensure_future))
except Exception as e:
    print('SKIP_Task_functions', type(e).__name__, e)

# === Event loop policy ===
try:
    print('abstracteventlooppolicy_class_exists', asyncio.AbstractEventLoopPolicy is not None)
    print('abstracteventlooppolicy_class_is_class', isinstance(asyncio.AbstractEventLoopPolicy, type))
    print('abstracteventloop_class_exists', asyncio.AbstractEventLoop is not None)
    print('baseeventloop_class_exists', asyncio.BaseEventLoop is not None)
except Exception as e:
    print('SKIP_Event_loop_policy', type(e).__name__, e)

# === Runner and TaskGroup ===
try:
    print('runner_class_exists', asyncio.Runner is not None)
    print('runner_class_is_class', isinstance(asyncio.Runner, type))
    print('taskgroup_class_exists', asyncio.TaskGroup is not None)
    print('taskgroup_class_is_class', isinstance(asyncio.TaskGroup, type))
except Exception as e:
    print('SKIP_Runner_and_TaskGroup', type(e).__name__, e)

# === Timeout ===
try:
    print('timeout_class_exists', asyncio.Timeout is not None)
    print('timeout_class_is_class', isinstance(asyncio.Timeout, type))
    print('timeout_function_exists', callable(asyncio.timeout))
    print('timeout_at_function_exists', callable(asyncio.timeout_at))
except Exception as e:
    print('SKIP_Timeout', type(e).__name__, e)

# === Streams ===
try:
    print('streamreader_class_exists', asyncio.StreamReader is not None)
    print('streamwriter_class_exists', asyncio.StreamWriter is not None)
    print('streamreaderprotocol_class_exists', asyncio.StreamReaderProtocol is not None)
except Exception as e:
    print('SKIP_Streams', type(e).__name__, e)

# === Protocols and Transports ===
try:
    print('baseprotocol_class_exists', asyncio.BaseProtocol is not None)
    print('protocol_class_exists', asyncio.Protocol is not None)
    print('datagramprotocol_class_exists', asyncio.DatagramProtocol is not None)
    print('subprocessprotocol_class_exists', asyncio.SubprocessProtocol is not None)
    print('bufferedprotocol_class_exists', asyncio.BufferedProtocol is not None)
    print('basetransport_class_exists', asyncio.BaseTransport is not None)
    print('transport_class_exists', asyncio.Transport is not None)
    print('readtransport_class_exists', asyncio.ReadTransport is not None)
    print('writetransport_class_exists', asyncio.WriteTransport is not None)
    print('datagramtransport_class_exists', asyncio.DatagramTransport is not None)
    print('subprocesstransport_class_exists', asyncio.SubprocessTransport is not None)
except Exception as e:
    print('SKIP_Protocols_and_Transports', type(e).__name__, e)

# === Server and Handle ===
try:
    print('server_class_exists', asyncio.Server is not None)
    print('abstractserver_class_exists', asyncio.AbstractServer is not None)
    print('handle_class_exists', asyncio.Handle is not None)
    print('timerhandle_class_exists', asyncio.TimerHandle is not None)
except Exception as e:
    print('SKIP_Server_and_Handle', type(e).__name__, e)

# === Other utility functions ===
try:
    print('run_function_exists', callable(asyncio.run))
    print('as_completed_function_exists', callable(asyncio.as_completed))
    print('new_event_loop_function_exists', callable(asyncio.new_event_loop))
    print('get_event_loop_function_exists', callable(asyncio.get_event_loop))
    print('set_event_loop_function_exists', callable(asyncio.set_event_loop))
    print('get_event_loop_policy_function_exists', callable(asyncio.get_event_loop_policy))
    print('set_event_loop_policy_function_exists', callable(asyncio.set_event_loop_policy))
    print('get_running_loop_function_exists', callable(asyncio.get_running_loop))
    print('to_thread_function_exists', callable(asyncio.to_thread))
except Exception as e:
    print('SKIP_Other_utility_functions', type(e).__name__, e)

# === Eager task factory ===
try:
    print('eager_task_factory_function_exists', callable(asyncio.eager_task_factory))
    print('create_eager_task_factory_function_exists', callable(asyncio.create_eager_task_factory))
except Exception as e:
    print('SKIP_Eager_task_factory', type(e).__name__, e)

# === Futures submodule ===
try:
    print('futures_submodule_exists', asyncio.futures is not None)
except Exception as e:
    print('SKIP_Futures_submodule', type(e).__name__, e)

# === Locks submodule ===
try:
    print('locks_submodule_exists', asyncio.locks is not None)
except Exception as e:
    print('SKIP_Locks_submodule', type(e).__name__, e)

# === Queues submodule ===
try:
    print('queues_submodule_exists', asyncio.queues is not None)
except Exception as e:
    print('SKIP_Queues_submodule', type(e).__name__, e)

# === Events submodule ===
try:
    print('events_submodule_exists', asyncio.events is not None)
except Exception as e:
    print('SKIP_Events_submodule', type(e).__name__, e)

# === Protocols submodule ===
try:
    print('protocols_submodule_exists', asyncio.protocols is not None)
except Exception as e:
    print('SKIP_Protocols_submodule', type(e).__name__, e)

# === Transports submodule ===
try:
    print('transports_submodule_exists', asyncio.transports is not None)
except Exception as e:
    print('SKIP_Transports_submodule', type(e).__name__, e)

# === Streams submodule ===
try:
    print('streams_submodule_exists', asyncio.streams is not None)
except Exception as e:
    print('SKIP_Streams_submodule', type(e).__name__, e)

# === Subprocess submodule ===
try:
    print('subprocess_submodule_exists', asyncio.subprocess is not None)
except Exception as e:
    print('SKIP_Subprocess_submodule', type(e).__name__, e)

# === Tasks submodule ===
try:
    print('tasks_submodule_exists', asyncio.tasks is not None)
except Exception as e:
    print('SKIP_Tasks_submodule', type(e).__name__, e)

# === Base events submodule ===
try:
    print('base_events_submodule_exists', asyncio.base_events is not None)
except Exception as e:
    print('SKIP_Base_events_submodule', type(e).__name__, e)

# === Base futures submodule ===
try:
    print('base_futures_submodule_exists', asyncio.base_futures is not None)
except Exception as e:
    print('SKIP_Base_futures_submodule', type(e).__name__, e)

# === Base tasks submodule ===
try:
    print('base_tasks_submodule_exists', asyncio.base_tasks is not None)
except Exception as e:
    print('SKIP_Base_tasks_submodule', type(e).__name__, e)

# === Exceptions submodule ===
try:
    print('exceptions_submodule_exists', asyncio.exceptions is not None)
except Exception as e:
    print('SKIP_Exceptions_submodule', type(e).__name__, e)
