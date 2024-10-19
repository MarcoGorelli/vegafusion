from __future__ import annotations

import sys
from types import ModuleType
from typing import TYPE_CHECKING, Any, Literal, TypedDict, Union, cast

import narwhals as nw
from arro3.core import Table

from vegafusion._vegafusion import get_cpu_count, get_virtual_memory
from vegafusion.transformer import DataFrameLike
from vegafusion.utils import get_inline_column_usage

from .local_tz import get_local_tz

if TYPE_CHECKING:
    import pandas as pd
    import polars as pl  # noqa: F401
    import pyarrow as pa
    from narwhals.typing import IntoFrameT

    from vegafusion._vegafusion import (
        PyChartState,
        PyChartStateGrpc,
        PyVegaFusionRuntime,
    )

# This type isn't defined in the grpcio package, so let's at least name it
UnaryUnaryMultiCallable = Any


def _get_common_namespace(inline_datasets: dict[str, Any] | None) -> str | None:
    namespaces = set()
    try:
        if inline_datasets is not None:
            for df in inline_datasets.values():
                namespaces.add(nw.get_native_namespace(nw.from_native(df)))

        if len(namespaces) == 1:
            return str(next(iter(namespaces)).__name__)
        else:
            return None
    except TypeError:
        # Types not compatible with Narwhals
        return None


def _get_default_namespace() -> ModuleType:
    # Returns a default narwhals namespace, based on what is installed
    if pd := sys.modules.get("pandas") and sys.modules.get("pyarrow"):
        return pd
    elif pl := sys.modules.get("polars"):
        return pl
    elif pa := sys.modules.get("pyarrow"):
        return pa
    else:
        raise ImportError("Could not determine default narwhals namespace")


class VariableUpdate(TypedDict):
    name: str
    namespace: Literal["data", "signal"]
    scope: list[int]
    value: Any


class Watch(TypedDict):
    name: str
    namespace: Literal["data", "signal"]
    scope: list[int]


class WatchPlan(TypedDict):
    client_to_server: list[Watch]
    server_to_client: list[Watch]


class PreTransformWarning(TypedDict):
    type: Literal["RowLimitExceeded", "BrokenInteractivity", "Unsupported"]
    message: str


class ChartState:
    def __init__(self, chart_state: PyChartState | PyChartStateGrpc) -> None:
        self._chart_state = chart_state

    def update(self, client_updates: list[VariableUpdate]) -> list[VariableUpdate]:
        """
        Update chart state with updates from the client.

        Args:
            client_updates: List of VariableUpdate values from the client.

        Returns:
            list of VariableUpdates that should be pushed to the client.
        """
        return cast(list[VariableUpdate], self._chart_state.update(client_updates))

    def get_watch_plan(self) -> WatchPlan:
        """
        Get ChartState's watch plan.

        Returns:
            WatchPlan specifying the signals and datasets that should be communicated
            between ChartState and client to preserve the input Vega spec's
            interactivity.
        """
        return cast(WatchPlan, self._chart_state.get_watch_plan())

    def get_transformed_spec(self) -> dict[str, Any]:
        """
        Get initial transformed spec.

        Returns:
            The initial transformed spec, equivalent to the spec produced by
            vf.runtime.pre_transform_spec().
        """
        return cast(dict[str, Any], self._chart_state.get_transformed_spec())

    def get_warnings(self) -> list[PreTransformWarning]:
        """Get transformed spec warnings

        Returns:
            list[PreTransformWarning]: A list of warnings as dictionaries.
                Each warning dict has a 'type' key indicating the warning type,
                and a 'message' key containing a description of the warning.

                Potential warning types include:
                    'RowLimitExceeded': Some datasets in resulting Vega specification
                        have been truncated to the provided row limit
                    'BrokenInteractivity': Some interactive features may have been
                        broken in the resulting Vega specification
                    'Unsupported': No transforms in the provided Vega specification were
                        eligible for pre-transforming
        """
        return cast(list[PreTransformWarning], self._chart_state.get_warnings())

    def get_server_spec(self) -> dict[str, Any]:
        """
        Returns:
            dict: The server spec.
        """
        return cast(dict[str, Any], self._chart_state.get_server_spec())

    def get_client_spec(self) -> dict[str, Any]:
        """
        Get client spec.

        Returns:
            dict: The client spec.
        """
        return cast(dict[str, Any], self._chart_state.get_client_spec())


class VegaFusionRuntime:
    def __init__(
        self,
        cache_capacity: int = 64,
        memory_limit: int | None = None,
        worker_threads: int | None = None,
    ) -> None:
        """
        Initialize a VegaFusionRuntime.

        Args:
            cache_capacity: Cache capacity.
            memory_limit: Memory limit.
            worker_threads: Number of worker threads.
        """
        self._runtime = None
        self._grpc_url: str | None = None
        self._cache_capacity = cache_capacity
        self._memory_limit = memory_limit
        self._worker_threads = worker_threads

    @property
    def runtime(self) -> PyVegaFusionRuntime:
        """
        Get or initialize a VegaFusion runtime.

        Returns:
            The VegaFusion runtime.
        """
        if self._runtime is None:
            # Try to initialize a VegaFusion runtime
            from vegafusion._vegafusion import PyVegaFusionRuntime

            if self.memory_limit is None:
                self.memory_limit = get_virtual_memory() // 2
            if self.worker_threads is None:
                self.worker_threads = get_cpu_count()

            self._runtime = PyVegaFusionRuntime.new_embedded(
                self.cache_capacity,
                self.memory_limit,
                self.worker_threads,
            )
        return self._runtime

    def grpc_connect(self, url: str) -> None:
        """
        Connect to a VegaFusion server over gRPC at the provided gRPC url

        Args:
            url: URL of a running VegaFusion server
        """
        from vegafusion._vegafusion import PyVegaFusionRuntime

        self._grpc_url = url
        self._runtime = PyVegaFusionRuntime.new_grpc(url)

    @property
    def using_grpc(self) -> bool:
        """
        Check if using gRPC.

        Returns:
            True if using gRPC, False otherwise.
        """
        return self._grpc_url is not None

    def _import_inline_datasets(
        self,
        inline_datasets: dict[str, IntoFrameT] | None = None,
        inline_dataset_usage: dict[str, list[str]] | None = None,
    ) -> dict[str, Table]:
        """
        Import or register inline datasets.

        Args:
            inline_datasets: A dictionary from dataset names to pandas DataFrames or
                pyarrow Tables. Inline datasets may be referenced by the input
                specification using the following url syntax
                'vegafusion+dataset://{dataset_name}' or 'table://{dataset_name}'.
            inline_dataset_usage: Columns that are referenced by datasets. If no
                entry is found, then all columns should be included.
        """
        if not TYPE_CHECKING:
            pd = sys.modules.get("pandas", None)
            pa = sys.modules.get("pyarrow", None)

        inline_datasets = inline_datasets or {}
        inline_dataset_usage = inline_dataset_usage or {}
        imported_inline_datasets: dict[str, Table] = {}
        for name, value in inline_datasets.items():
            columns = inline_dataset_usage.get(name)
            if pd is not None and pa is not None and isinstance(value, pd.DataFrame):
                # rename to help mypy
                inner_value: pd.DataFrame = value
                del value

                # Project down columns if possible
                if columns is not None:
                    inner_value = inner_value[columns]

                # Convert problematic object columns to strings
                for col, dtype in inner_value.dtypes.items():
                    if dtype.kind == "O":
                        try:
                            # See if the Table constructor can handle column by itself
                            col_tbl = Table(inner_value[[col]])

                            # If so, keep the arrow version so that it's more efficient
                            # to convert as part of the whole table later
                            inner_value = inner_value.assign(
                                **{
                                    col: pd.arrays.ArrowExtensionArray(
                                        pa.chunked_array(col_tbl.column(0))
                                    )
                                }
                            )
                        except TypeError:
                            # If the Table constructor can't handle the object column,
                            # convert the column to pyarrow strings
                            inner_value = inner_value.assign(
                                **{col: inner_value[col].astype("string[pyarrow]")}
                            )
                if hasattr(inner_value, "__arrow_c_stream__"):
                    # TODO: this requires pyarrow 14.0.0 or later
                    imported_inline_datasets[name] = Table(inner_value)
                else:
                    # Older pandas, convert through pyarrow
                    imported_inline_datasets[name] = Table(pa.from_pandas(inner_value))
            elif isinstance(value, dict):
                # Let narwhals import the dict using a default backend
                df_nw = nw.from_dict(value, native_namespace=_get_default_namespace())
                imported_inline_datasets[name] = Table(df_nw)
            else:
                # Import through PyCapsule interface on narwhals
                try:
                    df_nw = nw.from_native(value)

                    # Project down columns if possible
                    if columns is not None:
                        # TODO: Nice error message when column is not found
                        df_nw = df_nw[columns]  # type: ignore[index]

                    imported_inline_datasets[name] = Table(df_nw)  # type: ignore[arg-type]
                except TypeError:
                    # Not supported by Narwhals, try pycapsule interface directly
                    if hasattr(value, "__arrow_c_stream__"):
                        imported_inline_datasets[name] = Table(value)  # type: ignore[arg-type]
                    else:
                        raise

        return imported_inline_datasets

    def pre_transform_spec(
        self,
        spec: Union[dict[str, Any], str],
        local_tz: str | None = None,
        default_input_tz: str | None = None,
        row_limit: int | None = None,
        preserve_interactivity: bool = True,
        inline_datasets: dict[str, Any] | None = None,
        keep_signals: list[Union[str, tuple[str, list[int]]]] | None = None,
        keep_datasets: list[Union[str, tuple[str, list[int]]]] | None = None,
        data_encoding_threshold: int | None = None,
        data_encoding_format: str = "arro3",
    ) -> tuple[Union[dict[str, Any], str], list[dict[str, str]]]:
        """
        Evaluate supported transforms in an input Vega specification

        Produces a new specification with pre-transformed datasets included inline.

        Args:
            spec: A Vega specification dict or JSON string
            local_tz: Name of timezone to be considered local. E.g. 'America/New_York'.
                Defaults to the value of vf.get_local_tz(), which defaults to the system
                timezone if one can be determined.
            default_input_tz: Name of timezone (e.g. 'America/New_York') that naive
                datetime strings should be interpreted in. Defaults to `local_tz`.
            row_limit: Maximum number of dataset rows to include in the returned
                specification. If exceeded, datasets will be truncated to this number
                of rows and a RowLimitExceeded warning will be included in the
                resulting warnings list
            preserve_interactivity: If True (default) then the interactive behavior of
                the chart will pre preserved. This requires that all the data that
                participates in interactions be included in the resulting spec rather
                than being pre-transformed. If False, then all possible data
                transformations are applied even if they break the original interactive
                behavior of the chart.
            inline_datasets: A dict from dataset names to pandas DataFrames or pyarrow
                Tables. Inline datasets may be referenced by the input specification
                using the following url syntax 'vegafusion+dataset://{dataset_name}' or
                'table://{dataset_name}'.
            keep_signals: Signals from the input spec that must be included in the
                pre-transformed spec. A list with elements that are either:
                - The name of a top-level signal as a string
                - A two-element tuple where the first element is the name of a signal
                  as a string and the second element is the nested scope of the dataset
                  as a list of integers
            keep_datasets: Datasets from the input spec that must be included in the
                pre-transformed spec. A list with elements that are either:
                - The name of a top-level dataset as a string
                - A two-element tuple where the first element is the name of a dataset
                  as a string and the second element is the nested scope of the dataset
                  as a list of integers
            data_encoding_threshold: threshold for encoding datasets. When length of
                pre-transformed datasets exceeds data_encoding_threshold, datasets are
                encoded into an alternative format (as determined by the
                data_encoding_format argument). When None (the default),
                pre-transformed datasets are never encoded and are always included as
                JSON compatible lists of dictionaries.
            data_encoding_format: format of encoded datasets. Format to use to encode
                datasets with length exceeding the data_encoding_threshold argument.
                - "arro3": Encode datasets as arro3 Tables. Not JSON compatible.
                - "pyarrow": Encode datasets as pyarrow Tables. Not JSON compatible.
                - "arrow-ipc": Encode datasets as bytes in Arrow IPC format. Not JSON
                  compatible.
                - "arrow-ipc-base64": Encode datasets as strings in base64 encoded
                  Arrow IPC format. JSON compatible.

        Returns:
            A tuple containing:
            - A string containing the JSON representation of a Vega specification
              with pre-transformed datasets included inline
            - A list of warnings as dictionaries. Each warning dict has a 'type'
              key indicating the warning type, and a 'message' key containing
              a description of the warning. Potential warning types include:
                'RowLimitExceeded': Some datasets in resulting Vega specification
                    have been truncated to the provided row limit
                'BrokenInteractivity': Some interactive features may have been
                    broken in the resulting Vega specification
                'Unsupported': No transforms in the provided Vega specification were
                    eligible for pre-transforming
        """
        local_tz = local_tz or get_local_tz()
        imported_inline_dataset = self._import_inline_datasets(
            inline_datasets, get_inline_column_usage(spec)
        )

        if data_encoding_threshold is None:
            new_spec, warnings = self.runtime.pre_transform_spec(
                spec,
                local_tz=local_tz,
                default_input_tz=default_input_tz,
                row_limit=row_limit,
                preserve_interactivity=preserve_interactivity,
                inline_datasets=imported_inline_dataset,
                keep_signals=parse_variables(keep_signals),
                keep_datasets=parse_variables(keep_datasets),
            )
        else:
            # Use pre_transform_extract to extract large datasets
            new_spec, datasets, warnings = self.runtime.pre_transform_extract(
                spec,
                local_tz=local_tz,
                default_input_tz=default_input_tz,
                preserve_interactivity=preserve_interactivity,
                extract_threshold=data_encoding_threshold,
                extracted_format=data_encoding_format,
                inline_datasets=imported_inline_dataset,
                keep_signals=parse_variables(keep_signals),
                keep_datasets=parse_variables(keep_datasets),
            )

            # Insert encoded datasets back into spec
            for name, scope, tbl in datasets:
                group = get_mark_group_for_scope(new_spec, scope) or {}
                for data in group.get("data", []):
                    if data.get("name", None) == name:
                        data["values"] = tbl

        return new_spec, warnings

    def new_chart_state(
        self,
        spec: Union[dict[str, Any], str],
        local_tz: str | None = None,
        default_input_tz: str | None = None,
        row_limit: int | None = None,
        inline_datasets: dict[str, DataFrameLike] | None = None,
    ) -> ChartState:
        """Construct new ChartState object.

        Args:
            spec: A Vega specification dict or JSON string.
            local_tz: Name of timezone to be considered local. E.g. 'America/New_York'.
                Defaults to the value of vf.get_local_tz(), which defaults to the system
                timezone if one can be determined.
            default_input_tz: Name of timezone (e.g. 'America/New_York') that naive
                datetime strings should be interpreted in. Defaults to `local_tz`.
            row_limit: Maximum number of dataset rows to include in the returned
                datasets. If exceeded, datasets will be truncated to this number of
                rows and a RowLimitExceeded warning will be included in the ChartState's
                warnings list.
            inline_datasets: A dict from dataset names to pandas DataFrames or pyarrow
                Tables. Inline datasets may be referenced by the input specification
                using the following url syntax 'vegafusion+dataset://{dataset_name}' or
                'table://{dataset_name}'.

        Returns:
            ChartState object.
        """
        local_tz = local_tz or get_local_tz()
        inline_arrow_dataset = self._import_inline_datasets(
            inline_datasets, get_inline_column_usage(spec)
        )
        return ChartState(
            self.runtime.new_chart_state(
                spec, local_tz, default_input_tz, row_limit, inline_arrow_dataset
            )
        )

    def pre_transform_datasets(
        self,
        spec: Union[dict[str, Any], str],
        datasets: list[Union[str, tuple[str, list[int]]]],
        local_tz: str | None = None,
        default_input_tz: str | None = None,
        row_limit: int | None = None,
        inline_datasets: dict[str, DataFrameLike] | None = None,
        trim_unused_columns: bool = False,
    ) -> tuple[list[DataFrameLike], list[dict[str, str]]]:
        """Extract the fully evaluated form of the requested datasets from a Vega
        specification.

        Extracts datasets as pandas DataFrames.

        Args:
            spec: A Vega specification dict or JSON string.
            datasets: A list with elements that are either:
                - The name of a top-level dataset as a string
                - A two-element tuple where the first element is the name of a dataset
                  as a string and the second element is the nested scope of the dataset
                  as a list of integers
            local_tz: Name of timezone to be considered local. E.g. 'America/New_York'.
                Defaults to the value of vf.get_local_tz(), which defaults to the
                system timezone if one can be determined.
            default_input_tz: Name of timezone (e.g. 'America/New_York') that naive
                datetime strings should be interpreted in. Defaults to `local_tz`.
            row_limit: Maximum number of dataset rows to include in the returned
                datasets. If exceeded, datasets will be truncated to this number of
                rows and a RowLimitExceeded warning will be included in the resulting
                warnings list.
            inline_datasets: A dict from dataset names to pandas DataFrames or pyarrow
                Tables. Inline datasets may be referenced by the input specification
                using the following url syntax 'vegafusion+dataset://{dataset_name}'
                or 'table://{dataset_name}'.
            trim_unused_columns: If True, unused columns are removed from returned
                datasets.

        Returns:
            A tuple containing:
                - List of pandas DataFrames corresponding to the input datasets list
                - A list of warnings as dictionaries. Each warning dict has a 'type'
                  key indicating the warning type, and a 'message' key containing a
                  description of the warning.
        """
        if not TYPE_CHECKING:
            pl = sys.modules.get("polars", None)
            pa = sys.modules.get("pyarrow", None)
            pd = sys.modules.get("pandas", None)

        local_tz = local_tz or get_local_tz()

        # Build input variables
        pre_tx_vars = parse_variables(datasets)

        # Serialize inline datasets
        inline_arrow_dataset = self._import_inline_datasets(
            inline_datasets,
            inline_dataset_usage=get_inline_column_usage(spec)
            if trim_unused_columns
            else None,
        )

        values, warnings = self.runtime.pre_transform_datasets(
            spec,
            pre_tx_vars,
            local_tz=local_tz,
            default_input_tz=default_input_tz,
            row_limit=row_limit,
            inline_datasets=inline_arrow_dataset,
        )

        # Wrap result dataframes in native format, then with Narwhals so that
        # we can manipulate them with a uniform API
        namespace = _get_common_namespace(inline_datasets)
        if namespace == "polars" and pl is not None:
            nw_dataframes = [nw.from_native(pl.DataFrame(value)) for value in values]

        elif namespace == "pyarrow" and pa is not None:
            nw_dataframes = [nw.from_native(pa.table(value)) for value in values]
        elif namespace == "pandas" and pd is not None and pa is not None:
            nw_dataframes = [
                nw.from_native(pa.table(value).to_pandas()) for value in values
            ]
        else:
            # Either no inline datasets, inline datasets with mixed or
            # unrecognized types
            if pa is not None and pd is not None:
                nw_dataframes = [
                    nw.from_native(pa.table(value).to_pandas()) for value in values
                ]
            elif pl is not None:
                nw_dataframes = [
                    nw.from_native(pl.DataFrame(value)) for value in values
                ]
            else:
                # Hopefully narwhals will eventually help us fall back to whatever
                # is installed here
                raise ValueError(
                    "Either polars or pandas must be installed to extract "
                    "transformed data"
                )

        # Localize datetime columns to UTC, then extract the native DataFrame
        # to return
        processed_datasets = []
        for df in nw_dataframes:
            for name in df.columns:
                dtype = df[name].dtype
                if dtype == nw.Datetime:
                    df = df.with_columns(
                        df[name]
                        .dt.replace_time_zone("UTC")
                        .dt.convert_time_zone(local_tz)
                    )
            processed_datasets.append(df.to_native())

        return processed_datasets, warnings

    def pre_transform_extract(
        self,
        spec: dict[str, Any] | str,
        local_tz: str | None = None,
        default_input_tz: str | None = None,
        preserve_interactivity: bool = True,
        extract_threshold: int = 20,
        extracted_format: str = "arro3",
        inline_datasets: dict[str, DataFrameLike] | None = None,
        keep_signals: list[str | tuple[str, list[int]]] | None = None,
        keep_datasets: list[str | tuple[str, list[int]]] | None = None,
    ) -> tuple[
        dict[str, Any], list[tuple[str, list[int], pa.Table]], list[dict[str, str]]
    ]:
        """
        Evaluate supported transforms in an input Vega specification.

        Produces a new specification with small pre-transformed datasets (under 100
        rows) included inline and larger inline datasets (100 rows or more) extracted
        into pyarrow tables.

        Args:
            spec: A Vega specification dict or JSON string.
            local_tz: Name of timezone to be considered local. E.g. 'America/New_York'.
                Defaults to the value of vf.get_local_tz(), which defaults to the system
                timezone if one can be determined.
            default_input_tz: Name of timezone (e.g. 'America/New_York') that naive
                datetime strings should be interpreted in. Defaults to `local_tz`.
            preserve_interactivity: If True (default) then the interactive behavior of
                the chart will pre preserved. This requires that all the data that
                participates in interactions be included in the resulting spec rather
                than being pre-transformed. If False, then all possible data
                transformations are applied even if they break the original interactive
                behavior of the chart.
            extract_threshold: Datasets with length below extract_threshold will be
                inlined.
            extracted_format: The format for the extracted datasets. Options are:
                - "arro3": arro3.Table
                - "pyarrow": pyarrow.Table
                - "arrow-ipc": bytes in arrow IPC format
                - "arrow-ipc-base64": base64 encoded arrow IPC format
            inline_datasets: A dict from dataset names to pandas DataFrames or pyarrow
                Tables. Inline datasets may be referenced by the input specification
                using the following url syntax 'vegafusion+dataset://{dataset_name}' or
                'table://{dataset_name}'.
            keep_signals: Signals from the input spec that must be included in the
                pre-transformed spec. A list with elements that are either:
                - The name of a top-level signal as a string
                - A two-element tuple where the first element is the name of a signal as
                  a string and the second element is the nested scope of the dataset as
                  a list of integers
            keep_datasets: Datasets from the input spec that must be included in the
                pre-transformed spec. A list with elements that are either:
                - The name of a top-level dataset as a string
                - A two-element tuple where the first element is the name of a dataset
                  as a string and the second element is the nested scope of the dataset
                  as a list of integers

        Returns:
            A tuple containing three elements:
            1. A dict containing the JSON representation of the pre-transformed Vega
               specification without pre-transformed datasets included inline
            2. Extracted datasets as a list of three element tuples:
               - dataset name
               - dataset scope
               - pyarrow Table
            3. A list of warnings as dictionaries. Each warning dict has a 'type' key
               indicating the warning type, and a 'message' key containing a description
               of the warning. Potential warning types include:
               - 'Planner': Planner warning
        """
        local_tz = local_tz or get_local_tz()

        inline_arrow_dataset = self._import_inline_datasets(
            inline_datasets, get_inline_column_usage(spec)
        )

        new_spec, datasets, warnings = self.runtime.pre_transform_extract(
            spec,
            local_tz=local_tz,
            default_input_tz=default_input_tz,
            preserve_interactivity=preserve_interactivity,
            extract_threshold=extract_threshold,
            extracted_format=extracted_format,
            inline_datasets=inline_arrow_dataset,
            keep_signals=keep_signals,
            keep_datasets=keep_datasets,
        )

        return new_spec, datasets, warnings

    def patch_pre_transformed_spec(
        self,
        spec1: dict[str, Any] | str,
        pre_transformed_spec1: dict[str, Any] | str,
        spec2: dict[str, Any] | str,
    ) -> dict[str, Any] | None:
        """
        Attempt to patch a Vega spec returned by the pre_transform_spec method.

        This method tries to patch a Vega spec without rerunning the pre_transform_spec
        logic. When possible, this can be significantly faster than rerunning the
        pre_transform_spec method.

        Args:
            spec1: The input Vega spec to a prior call to pre_transform_spec.
            pre_transformed_spec1: The prior result of passing spec1 to
                pre_transform_spec.
            spec2: A Vega spec that is assumed to be a small delta compared to spec1.

        Returns:
            If the delta between spec1 and spec2 is in the portions of spec1 that were
            not modified by pre_transform_spec, then this delta can be applied cleanly
            to pre_transform_spec1 and the result is returned. If the delta cannot be
            applied cleanly, None is returned and spec2 should be passed through the
            pre_transform_spec method.
        """
        if self.using_grpc:
            raise ValueError("patch_pre_transformed_spec not yet supported over gRPC")
        else:
            pre_transformed_spec2 = self.runtime.patch_pre_transformed_spec(
                spec1, pre_transformed_spec1, spec2
            )
            return cast(dict[str, Any], pre_transformed_spec2)

    @property
    def worker_threads(self) -> int | None:
        """
        Get the number of worker threads for the runtime.

        Returns:
            Number of threads for the runtime
        """
        return self._worker_threads

    @worker_threads.setter
    def worker_threads(self, value: int) -> None:
        """
        Restart the runtime with the specified number of worker threads

        Args:
            value: Number of threads for the new runtime
        """
        if value != self._worker_threads:
            self._worker_threads = value
            self.reset()

    @property
    def total_memory(self) -> int | None:
        if self._runtime:
            return self._runtime.total_memory()
        else:
            return None

    @property
    def _protected_memory(self) -> int | None:
        if self._runtime:
            return self._runtime.protected_memory()
        else:
            return None

    @property
    def _probationary_memory(self) -> int | None:
        if self._runtime:
            return self._runtime.probationary_memory()
        else:
            return None

    @property
    def size(self) -> int | None:
        if self._runtime:
            return self._runtime.size()
        else:
            return None

    @property
    def memory_limit(self) -> int | None:
        return self._memory_limit

    @memory_limit.setter
    def memory_limit(self, value: int) -> None:
        """
        Restart the runtime with the specified memory limit

        Args:
            value: Max approximate memory usage of cache
        """
        if value != self._memory_limit:
            self._memory_limit = value
            self.reset()

    @property
    def cache_capacity(self) -> int:
        return self._cache_capacity

    @cache_capacity.setter
    def cache_capacity(self, value: int) -> None:
        """
        Restart the runtime with the specified cache capacity

        Args:
            value: Max task graph values to cache
        """
        if value != self._cache_capacity:
            self._cache_capacity = value
            self.reset()

    def reset(self) -> None:
        if self._runtime is not None:
            self._runtime.clear_cache()
            self._runtime = None

    def __repr__(self) -> str:
        if self.using_grpc:
            return f"VegaFusionRuntime(url={self._grpc_url})"
        else:
            return (
                f"VegaFusionRuntime(cache_capacity={self.cache_capacity}, "
                f"worker_threads={self.worker_threads})"
            )


def parse_variables(
    variables: list[str | tuple[str, list[int]]] | None,
) -> list[tuple[str, list[int]]]:
    """
    Parse VegaFusion variables.

    Args:
        variables: List of VegaFusion variables.

    Returns:
        List of parsed VegaFusion variables.
    """
    # Build input variables
    pre_tx_vars: list[tuple[str, list[int]]] = []
    if variables is None:
        return []

    if isinstance(variables, str):
        variables = [variables]

    err_msg = "Elements of variables argument must be strings or two-element tuples"
    for var in variables:
        if isinstance(var, str):
            pre_tx_vars.append((var, []))
        elif isinstance(var, (list, tuple)):
            if len(var) == 2:
                pre_tx_vars.append((var[0], list(var[1])))
            else:
                raise ValueError(err_msg)
        else:
            raise ValueError(err_msg)
    return pre_tx_vars


def get_mark_group_for_scope(
    vega_spec: dict[str, Any], scope: list[int]
) -> dict[str, Any] | None:
    group = vega_spec

    # Find group at scope
    for scope_value in scope:
        group_index = 0
        child_group = None
        for mark in group.get("marks", []):
            if mark.get("type") == "group":
                if group_index == scope_value:
                    child_group = mark
                    break
                group_index += 1
        if child_group is None:
            return None
        group = child_group

    return group


runtime = VegaFusionRuntime(64, None, None)
