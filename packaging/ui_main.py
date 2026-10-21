"""Entry point of DragShiftUI.exe, the editor half that DragShift.exe starts on demand."""
import sys

from dragshift.worker import main

if __name__ == "__main__":
    sys.exit(main())
