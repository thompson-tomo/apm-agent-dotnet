// <copyright file="BeginMethodHandler`4.cs" company="Datadog">
// Unless explicitly stated otherwise all files in this repository are licensed under the Apache 2 License.
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2017 Datadog, Inc.
// </copyright>
// Licensed to Elasticsearch B.V under one or more agreements.
// Elasticsearch B.V licenses this file to you under the Apache 2.0 License.
// See the LICENSE file in the project root for more information

using System;
using System.Reflection.Emit;
using System.Runtime.CompilerServices;

#pragma warning disable SA1649 // File name must match first type name

namespace Elastic.Apm.Profiler.Managed.CallTarget.Handlers
{
    internal static class BeginMethodHandler<TIntegration, TTarget, TArg1, TArg2, TArg3, TArg4>
    {
        private static readonly InvokeDelegate _invokeDelegate;

        static BeginMethodHandler()
        {
            try
            {
                var dynMethod = IntegrationMapper.CreateBeginMethodDelegate(typeof(TIntegration), typeof(TTarget), new[] { typeof(TArg1), typeof(TArg2), typeof(TArg3), typeof(TArg4) });
                if (dynMethod != null)
                {
                    _invokeDelegate = (InvokeDelegate)dynMethod.CreateDelegate(typeof(InvokeDelegate));
                }
            }
            catch (Exception ex)
            {
                throw new CallTargetInvokerException(ex);
            }
            finally
            {
                if (_invokeDelegate is null)
                {
                    _invokeDelegate = (instance, arg1, arg2, arg3, arg4) => CallTargetState.GetDefault();
                }
            }
        }

        internal delegate CallTargetState InvokeDelegate(TTarget instance, TArg1 arg1, TArg2 arg2, TArg3 arg3, TArg4 arg4);

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        internal static CallTargetState Invoke(TTarget instance, TArg1 arg1, TArg2 arg2, TArg3 arg3, TArg4 arg4) =>
			new CallTargetState(Agent.Tracer.CurrentExecutionSegment(), _invokeDelegate(instance, arg1, arg2, arg3, arg4));
	}
}