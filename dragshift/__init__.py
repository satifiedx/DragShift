import os

__version__ = "2.2.0"

# numpy's OpenBLAS commits a buffer per CPU thread on import (~350 MB private on 12 threads).
# DragShift never does linear algebra, so one thread is plenty.
os.environ.setdefault("OPENBLAS_NUM_THREADS", "1")
